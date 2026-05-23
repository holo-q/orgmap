//! `org screen` — anti-sloppy normalizer for the org's project tree.
//!
//! Walks every local project in the institution graph, runs a baked-in pattern
//! set over the tracked file list (`git ls-files`), and emits a severity-rolled
//! report of secrets, personal-identity leaks, and pre-publish sloppiness.
//!
//! Three pattern families, three severity tiers:
//!   - Secrets  (Critical)  API keys, tokens, private-key PEM blocks
//!   - PathLeak (Warn)      `/home/<user>`, personal handles, emails
//!   - Sloppy   (Info)      dbg!/console.log/XXX/hardcoded localhost
//!
//! When `gitleaks` is on $PATH and `--no-gitleaks` was not passed, the
//! gitleaks `detect` results are merged in as additional Critical findings —
//! orthogonal coverage on top of the baked-in patterns.
//!
//! Allowlisting goes through `[screen]` in orgmap.toml:
//!   - personal_paths       — what counts as a path leak
//!   - personal_handles     — what counts as a handle/email leak
//!   - global_allow.{files,doc_extensions,patterns} — applied to every project
//!   - allow.<project>.{...} — per-project escapes

use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use regex::Regex;
use serde::Serialize;

use crate::institution::{Institution, Project, ScreenAllow, ScreenConfig};

/// Cap on file size we'll scan. Files above this skip — secrets in
/// multi-MB blobs are vanishingly rare and reading them stalls the walk.
const MAX_FILE_BYTES: u64 = 512 * 1024;

/// Bytes inspected for binary-ness. A NUL in this window classifies the
/// file as binary and skips line-by-line scanning.
const BINARY_SNIFF_BYTES: usize = 4096;

/// Maximum findings retained per project. Above this we drop the tail
/// and emit a synthetic `tracked_build_artifacts` summary — keeps the
/// terminal report scannable when a repo committed `target/` or similar.
const MAX_FINDINGS_PER_PROJECT: usize = 40;

/// Public alias for the printer in `main.rs` — keeps the constant
/// authoritative here while still being referenceable from the CLI.
pub const MAX_FINDINGS_PER_PROJECT_PUBLIC: usize = MAX_FINDINGS_PER_PROJECT;

/// Path prefixes that always skip — committed build artifacts, vendor
/// blobs, and language caches leak `/home/<user>/.cargo/...` strings
/// from compiler metadata and drown the signal. Their presence as
/// tracked files is itself a finding we surface separately.
const HARD_SKIP_PREFIXES: &[&str] = &[
    "target/",
    "node_modules/",
    ".next/",
    "dist/",
    "build/",
    "__pycache__/",
    ".venv/",
    "venv/",
    ".tox/",
    ".gradle/",
    ".idea/",
    "vendor/",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warn,
    Critical,
}

impl Severity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Critical => "secret",
            Self::Warn => "pathleak",
            Self::Info => "sloppy",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Self::Critical => "❌",
            Self::Warn => "⚠️ ",
            Self::Info => "ℹ️ ",
        }
    }

    pub fn ansi(self) -> u8 {
        match self {
            Self::Critical => 196,
            Self::Warn => 214,
            Self::Info => 39,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub project: String,
    pub file: String,
    pub line: usize,
    pub pattern_id: String,
    pub severity: Severity,
    pub snippet: String,
    pub source: FindingSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSource {
    Pattern,
    Gitleaks,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectFindings {
    pub name: String,
    pub path: PathBuf,
    pub critical: usize,
    pub warn: usize,
    pub info: usize,
    pub findings: Vec<Finding>,
    /// Number of findings dropped after MAX_FINDINGS_PER_PROJECT — surfaces
    /// as a `… + N more` tail in the terminal report.
    pub truncated: usize,
    /// Tracked files whose paths start with a HARD_SKIP_PREFIXES entry —
    /// the repo committed `target/`, `node_modules/`, etc. Their presence
    /// is a hygiene finding even though we don't scan them.
    pub tracked_build_artifacts: usize,
    /// Set when gitleaks was supposed to run for this project but bailed
    /// (binary not found, non-git tree, gitleaks itself errored). Surfaces
    /// in the terminal report so a clean scan can't be confused with a
    /// silent skip.
    pub gitleaks_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScreenReport {
    pub gh_org: String,
    pub root: PathBuf,
    pub projects_scanned: usize,
    pub projects_skipped: usize,
    pub critical: usize,
    pub warn: usize,
    pub info: usize,
    pub gitleaks_available: bool,
    pub projects: Vec<ProjectFindings>,
}

impl ScreenReport {
    pub fn exit_code(&self) -> i32 {
        if self.critical > 0 || self.warn > 0 {
            1
        } else {
            0
        }
    }
}

#[derive(Debug, Clone)]
pub struct ScreenOptions {
    /// When true, skip the gitleaks shell-out even if the binary is present.
    pub no_gitleaks: bool,
    /// When true, only emit Critical findings (silence pathleak + sloppy).
    pub secrets_only: bool,
}

impl Default for ScreenOptions {
    fn default() -> Self {
        Self {
            no_gitleaks: false,
            secrets_only: false,
        }
    }
}

/// Baked-in pattern definition. The id is the stable handle used by
/// allowlists.
struct Pattern {
    id: &'static str,
    severity: Severity,
    regex: Regex,
}

fn build_static_patterns() -> Vec<Pattern> {
    // Each regex is anchored by a high-entropy prefix; we deliberately
    // accept some false positives in exchange for not needing entropy
    // analysis. The user has gitleaks for the entropy lane.
    let raw: &[(&str, Severity, &str)] = &[
        // Secrets — Critical
        ("anthropic_key", Severity::Critical, r"sk-ant-[A-Za-z0-9_\-]{20,}"),
        ("openai_key", Severity::Critical, r"sk-(?:proj-)?[A-Za-z0-9]{32,}"),
        ("github_token", Severity::Critical, r"gh[pousr]_[A-Za-z0-9]{20,}"),
        ("aws_access_key", Severity::Critical, r"AKIA[0-9A-Z]{16}"),
        ("google_api_key", Severity::Critical, r"AIza[0-9A-Za-z\-_]{35}"),
        ("slack_token", Severity::Critical, r"xox[bpsaroe]-[A-Za-z0-9\-]{10,}"),
        ("private_key_block", Severity::Critical, r"-----BEGIN [A-Z ]*PRIVATE KEY-----"),
        ("jwt_token", Severity::Critical, r"eyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}"),

        // Sloppy — Info
        // Intentionally NOT flagged: console.log (legitimate JS/TS runtime
        // logger), TODO/FIXME/HACK/XXX (normal engineering markers),
        // localhost URLs (dev/preview tooling, test fixtures, and proxy
        // configs reference them legitimately). The screen focuses on
        // constructs that are unambiguously pre-publish concerns.
        ("dbg_macro", Severity::Info, r"\bdbg!\s*\("),
        ("abs_cargo_path", Severity::Info, r#"path\s*=\s*"/(?:home|Users)/"#),
    ];
    raw.iter()
        .map(|(id, sev, re)| Pattern {
            id,
            severity: *sev,
            regex: Regex::new(re).expect("baked-in screen regex must compile"),
        })
        .collect()
}

/// Patterns derived at runtime from `[screen].personal_paths` and
/// `personal_handles`. Returned alongside the static set so the scanner
/// only walks the file once.
fn build_dynamic_patterns(cfg: &ScreenConfig) -> Vec<Pattern> {
    let mut out = Vec::new();

    // Personal paths — default to $HOME if list is empty so the screen is
    // useful out-of-the-box without orgmap configuration.
    let default_home;
    let paths: &[String] = if cfg.personal_paths.is_empty() {
        if let Some(home) = std::env::var_os("HOME") {
            default_home = vec![home.to_string_lossy().into_owned()];
            &default_home
        } else {
            &[]
        }
    } else {
        &cfg.personal_paths
    };
    for path in paths {
        if path.is_empty() {
            continue;
        }
        let pattern = format!(r"{}", regex::escape(path));
        if let Ok(regex) = Regex::new(&pattern) {
            out.push(Pattern {
                // Leak strings collapse into one shared id so allowlists
                // address "all personal-path findings" without listing each
                // path. Same shape for handles below.
                id: "personal_path",
                severity: Severity::Warn,
                regex,
            });
        }
    }

    for handle in &cfg.personal_handles {
        if handle.is_empty() {
            continue;
        }
        if let Ok(regex) = Regex::new(&regex::escape(handle)) {
            out.push(Pattern {
                id: "personal_handle",
                severity: Severity::Warn,
                regex,
            });
        }
    }

    out
}

/// Entry point — walk the institution and produce a populated report.
pub fn run(institution: &Institution, opts: &ScreenOptions) -> ScreenReport {
    let static_patterns = build_static_patterns();
    let dynamic_patterns = build_dynamic_patterns(&institution_screen_config(institution));
    let cfg = institution_screen_config(institution);

    let gitleaks_available = !opts.no_gitleaks && which_gitleaks().is_some();

    let mut report = ScreenReport {
        gh_org: institution.gh_org.clone(),
        root: institution.root.clone(),
        projects_scanned: 0,
        projects_skipped: 0,
        critical: 0,
        warn: 0,
        info: 0,
        gitleaks_available,
        projects: Vec::new(),
    };

    for project in &institution.projects {
        if project.blacklisted || !project.local {
            report.projects_skipped += 1;
            continue;
        }
        let Some(path) = project.path.as_ref() else {
            report.projects_skipped += 1;
            continue;
        };
        let pf = scan_project(
            project,
            path,
            &cfg,
            &static_patterns,
            &dynamic_patterns,
            gitleaks_available,
            opts,
        );
        report.critical += pf.critical;
        report.warn += pf.warn;
        report.info += pf.info;
        report.projects_scanned += 1;
        report.projects.push(pf);
    }

    // Sort projects by severity descending, then by name — most-broken
    // first so the operator's eye lands on real problems.
    report.projects.sort_by(|a, b| {
        let a_key = (a.critical, a.warn, a.info);
        let b_key = (b.critical, b.warn, b.info);
        b_key.cmp(&a_key).then_with(|| a.name.cmp(&b.name))
    });

    report
}

fn institution_screen_config(institution: &Institution) -> ScreenConfig {
    // Re-read the orgmap.toml to pluck the [screen] block. The Institution
    // type doesn't carry it through (loader path predates this feature) —
    // a re-parse here keeps the load contract stable while letting screen
    // self-configure. Cheap (small TOML, one-shot).
    let Ok(text) = std::fs::read_to_string(&institution.config_path) else {
        return ScreenConfig::default();
    };
    let parsed: Result<crate::institution::OrgConfig, _> = toml::from_str(&text);
    parsed.map(|cfg| cfg.screen).unwrap_or_default()
}

fn scan_project(
    project: &Project,
    project_path: &Path,
    cfg: &ScreenConfig,
    static_patterns: &[Pattern],
    dynamic_patterns: &[Pattern],
    gitleaks_available: bool,
    opts: &ScreenOptions,
) -> ProjectFindings {
    let mut pf = ProjectFindings {
        name: project.name.clone(),
        path: project_path.to_path_buf(),
        critical: 0,
        warn: 0,
        info: 0,
        findings: Vec::new(),
        truncated: 0,
        tracked_build_artifacts: 0,
        gitleaks_error: None,
    };

    // Merge allow rules: global first, project-specific overrides on top.
    let allow = merge_allow(&cfg.global_allow, cfg.allow.get(&project.name));

    let Some(files) = git_ls_files(project_path) else {
        // Not a git tree (or git error) — treat as a skip, log nothing.
        return pf;
    };

    for rel in files {
        let rel_str = rel.to_string_lossy();
        // Hard-skip well-known build-artifact directories. Their tracking
        // is itself a finding (surfaced once as `tracked_build_artifacts`)
        // but scanning them would flood the report with compiler-metadata
        // path leaks that aren't the project's fault.
        if HARD_SKIP_PREFIXES
            .iter()
            .any(|prefix| rel_str.starts_with(prefix) || rel_str.contains(&format!("/{prefix}")))
        {
            pf.tracked_build_artifacts += 1;
            continue;
        }
        if path_in_allow(&rel, &allow.files) {
            continue;
        }
        let abs = project_path.join(&rel);
        let Some(text) = read_text_file(&abs) else {
            continue;
        };
        let is_doc = extension_is_doc(&rel, &allow.doc_extensions);

        scan_text(
            &project.name,
            &rel,
            &text,
            static_patterns,
            dynamic_patterns,
            &allow.patterns,
            is_doc,
            opts.secrets_only,
            &mut pf,
        );
    }

    // Promote committed-build-artifacts into a single Warn-level finding.
    // The screen exit gate trips on this — committing target/ or
    // node_modules/ is sloppy enough to block publish. Honor
    // secrets_only by skipping the synthetic Warn entirely in that mode.
    if pf.tracked_build_artifacts > 0 && !opts.secrets_only {
        let synthetic = Finding {
            project: project.name.clone(),
            file: "<tree>".to_string(),
            line: 0,
            pattern_id: "tracked_build_artifacts".to_string(),
            severity: Severity::Warn,
            snippet: format!(
                "{} tracked files under build-artifact dirs (target/, node_modules/, dist/, …) — gitignore them",
                pf.tracked_build_artifacts
            ),
            source: FindingSource::Pattern,
        };
        record_finding(&mut pf, synthetic);
    }

    if gitleaks_available {
        match run_gitleaks(project_path) {
            Ok(findings) => {
                for finding in findings {
                    if path_in_allow(Path::new(&finding.file), &allow.files) {
                        continue;
                    }
                    if allow.patterns.iter().any(|p| p == &finding.pattern_id) {
                        continue;
                    }
                    record_finding(&mut pf, finding);
                }
            }
            Err(message) => {
                pf.gitleaks_error = Some(message);
            }
        }
    }

    pf
}

fn merge_allow(global: &ScreenAllow, project: Option<&ScreenAllow>) -> ScreenAllow {
    let mut out = global.clone();
    if let Some(p) = project {
        out.files.extend(p.files.iter().cloned());
        out.doc_extensions.extend(p.doc_extensions.iter().cloned());
        out.patterns.extend(p.patterns.iter().cloned());
    }
    out
}

fn path_in_allow(rel: &Path, allow_files: &[String]) -> bool {
    let rel_str = rel.to_string_lossy();
    allow_files.iter().any(|prefix| {
        // Prefix match — `docs/` allows everything under docs, `AGENTS.md`
        // allows exactly that file.
        rel_str == prefix.as_str() || rel_str.starts_with(prefix.as_str())
    })
}

fn extension_is_doc(rel: &Path, doc_exts: &[String]) -> bool {
    let default: &[&str] = &["md", "markdown", "txt", "rst"];
    let user: Vec<&str> = doc_exts
        .iter()
        .map(|s| s.trim_start_matches('.'))
        .collect();
    let ext = rel
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    if ext.is_empty() {
        return false;
    }
    default.iter().any(|d| *d == ext) || user.iter().any(|d| *d == ext.as_str())
}

fn scan_text(
    project: &str,
    rel: &Path,
    text: &str,
    static_patterns: &[Pattern],
    dynamic_patterns: &[Pattern],
    suppressed_patterns: &[String],
    is_doc: bool,
    secrets_only: bool,
    pf: &mut ProjectFindings,
) {
    let rel_str = rel.to_string_lossy().into_owned();
    for (line_no, line) in text.lines().enumerate() {
        // Cheap line-length guard — extremely long lines (minified JS,
        // base64 blobs) are scanned but truncated for the snippet.
        for pattern in static_patterns.iter().chain(dynamic_patterns.iter()) {
            if suppressed_patterns.iter().any(|p| p == pattern.id) {
                continue;
            }
            let Some(mat) = pattern.regex.find(line) else {
                continue;
            };
            let mut severity = pattern.severity;
            // Docs (markdown, etc.) downgrade pathleak Warn to Info — the
            // README is the right place to *say* `/home/nuck` exists; the
            // wrong place is hardcoded source. Secrets never downgrade.
            if is_doc && severity == Severity::Warn {
                severity = Severity::Info;
            }
            if secrets_only && severity != Severity::Critical {
                continue;
            }
            let snippet = snippet_around(line, mat.start(), mat.end());
            let finding = Finding {
                project: project.to_string(),
                file: rel_str.clone(),
                line: line_no + 1,
                pattern_id: pattern.id.to_string(),
                severity,
                snippet,
                source: FindingSource::Pattern,
            };
            record_finding(pf, finding);
        }
    }
}

fn record_finding(pf: &mut ProjectFindings, finding: Finding) {
    // Always tally the counts — they're authoritative regardless of
    // whether the underlying Finding row survives the truncation cap.
    match finding.severity {
        Severity::Critical => pf.critical += 1,
        Severity::Warn => pf.warn += 1,
        Severity::Info => pf.info += 1,
    }
    // Critical findings always make it into the surfaced list — they're
    // why this tool exists. Warn/Info drop into the truncated bucket once
    // the cap is hit.
    if finding.severity == Severity::Critical || pf.findings.len() < MAX_FINDINGS_PER_PROJECT {
        pf.findings.push(finding);
    } else {
        pf.truncated += 1;
    }
}

fn snippet_around(line: &str, start: usize, end: usize) -> String {
    const CTX: usize = 24;
    const MAX_LEN: usize = 120;
    let line_bytes = line.as_bytes();
    let from = start.saturating_sub(CTX);
    let to = (end + CTX).min(line_bytes.len());
    let from = char_boundary(line, from);
    let to = char_boundary(line, to);
    let mut s = line[from..to].to_string();
    if s.len() > MAX_LEN {
        s.truncate(MAX_LEN);
        s.push('…');
    }
    s
}

fn char_boundary(s: &str, mut idx: usize) -> usize {
    while idx > 0 && !s.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

fn read_text_file(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.len() > MAX_FILE_BYTES {
        return None;
    }
    let bytes = std::fs::read(path).ok()?;
    let sniff_end = bytes.len().min(BINARY_SNIFF_BYTES);
    if bytes[..sniff_end].contains(&0u8) {
        return None;
    }
    String::from_utf8(bytes).ok()
}

fn git_ls_files(project_path: &Path) -> Option<Vec<PathBuf>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(project_path)
        .arg("ls-files")
        .arg("-z")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        output
            .stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|bytes| PathBuf::from(String::from_utf8_lossy(bytes).into_owned()))
            .collect(),
    )
}

fn which_gitleaks() -> Option<PathBuf> {
    // Cheap PATH walk so we don't shell out to `which` itself.
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join("gitleaks");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// JSON shape emitted by `gitleaks detect --report-format json`. Only the
/// fields we surface are deserialized; gitleaks ships many more.
#[derive(Debug, serde::Deserialize)]
struct GitleaksRecord {
    #[serde(rename = "RuleID", default)]
    rule_id: String,
    #[serde(rename = "File", default)]
    file: String,
    #[serde(rename = "StartLine", default)]
    start_line: usize,
    #[serde(rename = "Match", default)]
    match_text: String,
    #[serde(rename = "Secret", default)]
    secret: String,
}

fn run_gitleaks(project_path: &Path) -> Result<Vec<Finding>, String> {
    // Write the report to a temp file rather than stdout — gitleaks
    // mingles human-readable progress chatter on stdout/stderr and the
    // `--report-path -` contract has shifted between versions. A
    // temp-file dance is the stable shape.
    let report_dir = std::env::temp_dir();
    let report_path = report_dir.join(format!(
        "orgmap-gitleaks-{}.json",
        std::process::id()
    ));
    // Detect with --no-banner + JSON report. Walks history by default —
    // we want that: rotated-but-committed keys are still leaks.
    let output = Command::new("gitleaks")
        .arg("detect")
        .arg("--source")
        .arg(project_path)
        .arg("--no-banner")
        .arg("--redact")
        .arg("--report-format")
        .arg("json")
        .arg("--report-path")
        .arg(&report_path)
        .arg("--exit-code")
        .arg("0") // never let gitleaks itself fail the process — we read its findings
        .output()
        .map_err(|e| format!("gitleaks spawn failed: {e}"))?;

    if !output.status.success() && !report_path.exists() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gitleaks errored: {}", stderr.trim()));
    }

    let raw = std::fs::read_to_string(&report_path).map_err(|e| format!("read report: {e}"))?;
    let _ = std::fs::remove_file(&report_path);
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let records: Vec<GitleaksRecord> =
        serde_json::from_str(&raw).map_err(|e| format!("parse report: {e}"))?;
    let project_name = project_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_string();
    Ok(records
        .into_iter()
        .map(|r| {
            let snippet = if !r.secret.is_empty() {
                r.secret
            } else {
                r.match_text
            };
            Finding {
                project: project_name.clone(),
                file: relative_or_self(project_path, &r.file),
                line: r.start_line,
                pattern_id: format!("gitleaks:{}", r.rule_id),
                severity: Severity::Critical,
                snippet,
                source: FindingSource::Gitleaks,
            }
        })
        .collect())
}

fn relative_or_self(project_path: &Path, absolute_or_relative: &str) -> String {
    let p = Path::new(absolute_or_relative);
    if let Ok(stripped) = p.strip_prefix(project_path) {
        stripped.display().to_string()
    } else {
        absolute_or_relative.to_string()
    }
}

/// Group findings inside a project by (file, severity) for terminal display
/// — keeps rows tight when one file has a dozen findings of the same kind.
pub fn group_by_file(project: &ProjectFindings) -> BTreeMap<String, Vec<&Finding>> {
    let mut out: BTreeMap<String, Vec<&Finding>> = BTreeMap::new();
    for f in &project.findings {
        out.entry(f.file.clone()).or_default().push(f);
    }
    for findings in out.values_mut() {
        findings.sort_by(|a, b| b.severity.cmp(&a.severity).then_with(|| a.line.cmp(&b.line)));
    }
    out
}
