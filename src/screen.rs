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
//! Beyond the baked universal patterns, orgs extend the screen two ways
//! (see docs/screen-extensibility.md): declarative `[[screen.pattern]]`
//! regexes (Lane A), and `[[screen.screener]]` external commands whose
//! findings are ingested via an adapter (Lane B). gitleaks is the first
//! built-in screener — auto-registered when present, NOT a special case in
//! the engine.
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

use crate::institution::{Institution, Project, ScreenAllow, ScreenConfig, ScreenScreener};

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FindingSource {
    /// A baked-in or org-declared regex pattern matched the line.
    Pattern,
    /// An external screener (Lane B) produced this finding; carries the
    /// screener's name for provenance + report labeling. gitleaks is one such.
    Screener(String),
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
    /// Per-screener errors (keyed by screener name) for screeners that were
    /// supposed to run for this project but bailed (binary errored, bad
    /// output, nonzero exit). Surfaces in the terminal report so a clean scan
    /// can't be confused with a silent skip.
    pub screener_errors: BTreeMap<String, String>,
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
    /// Names of the screeners (Lane B) that resolved and ran this pass —
    /// built-in + configured, minus `--skip-screener` and missing-optional.
    pub screeners_active: Vec<String>,
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

#[derive(Debug, Clone, Default)]
pub struct ScreenOptions {
    /// Screener names to skip this run (`--skip-screener`, plus the
    /// `--no-gitleaks` alias which pushes "gitleaks").
    pub skip_screeners: Vec<String>,
    /// When true, only emit Critical findings (silence pathleak + sloppy).
    pub secrets_only: bool,
    /// Run `deep` screeners (e.g. gitleaks' full-history walk). Set by main
    /// when the scan is project-scoped OR `--deep` was passed; a bare org-wide
    /// scan leaves this false so expensive screeners don't multiply across
    /// every project.
    pub run_deep: bool,
}

/// A compiled screening pattern. `id` is the stable handle used by
/// allowlists; baked-in patterns and org-declared `[[screen.pattern]]`
/// entries share this shape.
struct Pattern {
    id: String,
    severity: Severity,
    regex: Regex,
    /// Downgrade a `Warn` match to `Info` inside doc files. Universal
    /// pathleak patterns set this; secrets (Critical) ignore it.
    docs_downgrade: bool,
    /// Survive the `--secrets-only` gate even when not `Critical`.
    secrets_only_survives: bool,
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
            id: id.to_string(),
            severity: *sev,
            regex: Regex::new(re).expect("baked-in screen regex must compile"),
            // Universal behavior: Warn-tier leaks downgrade to advisory in
            // docs; nothing baked survives --secrets-only except by being
            // Critical (which the gate already lets through).
            docs_downgrade: *sev == Severity::Warn,
            secrets_only_survives: false,
        })
        .collect()
}

/// Parse a config severity string into a `Severity`, defaulting unknown/empty
/// to `Warn`. Accepts the tier labels as aliases so config can say either
/// `critical` or `secret`.
fn severity_from_str(s: &str) -> Severity {
    match s.trim().to_ascii_lowercase().as_str() {
        "critical" | "secret" | "error" => Severity::Critical,
        "info" | "sloppy" | "advisory" => Severity::Info,
        _ => Severity::Warn,
    }
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
                id: "personal_path".to_string(),
                severity: Severity::Warn,
                regex,
                docs_downgrade: true,
                secrets_only_survives: false,
            });
        }
    }

    for handle in &cfg.personal_handles {
        if handle.is_empty() {
            continue;
        }
        // Word-bounded: a handle like "nuck" must match as a whole token, not as
        // a substring of "ryunuck"/"canuck"/etc. Paths (above) stay unbounded —
        // a path fragment legitimately matches inside a longer path. `\b` sits on
        // the handle's outer word chars (names/emails start+end word-char), so
        // interior `@`/`.` are unaffected.
        if let Ok(regex) = Regex::new(&format!(r"\b{}\b", regex::escape(handle))) {
            out.push(Pattern {
                id: "personal_handle".to_string(),
                severity: Severity::Warn,
                regex,
                docs_downgrade: true,
                secrets_only_survives: false,
            });
        }
    }

    // Lane A — org-declared `[[screen.pattern]]` entries. Pure regex, appended
    // to the universal set. A bad regex is a config error: warn to stderr and
    // skip the entry rather than poisoning the whole scan.
    for p in &cfg.patterns {
        if p.id.is_empty() || p.regex.is_empty() {
            continue;
        }
        match Regex::new(&p.regex) {
            Ok(regex) => out.push(Pattern {
                id: p.id.clone(),
                severity: severity_from_str(&p.severity),
                regex,
                docs_downgrade: p.docs_downgrade,
                secrets_only_survives: p.secrets_only,
            }),
            Err(e) => eprintln!(
                "org screen: skipping pattern '{}' — invalid regex: {e}",
                p.id
            ),
        }
    }

    out
}

/// Entry point — walk the institution and produce a populated report.
pub fn run(institution: &Institution, opts: &ScreenOptions) -> ScreenReport {
    let cfg = institution_screen_config(institution);
    let static_patterns = build_static_patterns();
    let dynamic_patterns = build_dynamic_patterns(&cfg);

    // Build the screener registry (configured + auto-injected gitleaks), drop
    // `--skip-screener`'d names, then resolve binaries: optional+missing are
    // dropped silently; required+missing stay so run_screener surfaces the
    // error per project rather than silently no-op'ing.
    let screeners = resolve_screeners(&cfg, &institution.root, opts);
    let screeners_active: Vec<String> = screeners.iter().map(|s| s.name.clone()).collect();

    let mut report = ScreenReport {
        gh_org: institution.gh_org.clone(),
        root: institution.root.clone(),
        projects_scanned: 0,
        projects_skipped: 0,
        critical: 0,
        warn: 0,
        info: 0,
        screeners_active,
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
            &screeners,
            &institution.root,
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

/// Screen a single git repo at `path` DIRECTLY, bypassing orgmap project
/// resolution. `org screen <project>` only scans projects registered under the
/// scan roots, so it silently skips nested-workgroup repos (e.g.
/// `repo-com/ratatui/ratatui-ffi`). This path mode (`org screen .`) screens
/// whatever git tree it's pointed at — the basis of the universal pre-push hook.
/// The nearest `institution` still supplies the `[screen]` config (allowlists,
/// personal paths) and screener registry so results match a registered scan.
pub fn run_path(institution: &Institution, path: &Path, opts: &ScreenOptions) -> ScreenReport {
    let cfg = institution_screen_config(institution);
    let static_patterns = build_static_patterns();
    let dynamic_patterns = build_dynamic_patterns(&cfg);
    let screeners = resolve_screeners(&cfg, &institution.root, opts);
    let screeners_active: Vec<String> = screeners.iter().map(|s| s.name.clone()).collect();

    // Name the synthetic project after the repo dir (used for per-project allow
    // lookups + the report row label).
    let name = path
        .canonicalize()
        .ok()
        .as_deref()
        .and_then(Path::file_name)
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    let project = Project {
        name: name.clone(),
        display_name: name,
        workspace: String::new(),
        section: String::new(),
        path: Some(path.to_path_buf()),
        stage: None,
        description: None,
        tagline: None,
        local: true,
        blacklisted: false,
        git: None,
    };

    let pf = scan_project(
        &project,
        path,
        &cfg,
        &static_patterns,
        &dynamic_patterns,
        &screeners,
        &institution.root,
        opts,
    );
    ScreenReport {
        gh_org: institution.gh_org.clone(),
        root: institution.root.clone(),
        projects_scanned: 1,
        projects_skipped: 0,
        critical: pf.critical,
        warn: pf.warn,
        info: pf.info,
        screeners_active,
        projects: vec![pf],
    }
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
    screeners: &[Screener],
    org_root: &Path,
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
        screener_errors: BTreeMap::new(),
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

    // Lane B — run each active external screener over the project and ingest
    // its findings, applying the same allowlist + secrets-only gates the
    // pattern lane uses. A screener that bails records a per-name error so the
    // report can't mistake a broken screener for a clean scan.
    for screener in screeners {
        match run_screener(screener, project_path, org_root) {
            Ok(findings) => {
                for finding in findings {
                    if path_in_allow(Path::new(&finding.file), &allow.files) {
                        continue;
                    }
                    if allow.patterns.iter().any(|p| p == &finding.pattern_id) {
                        continue;
                    }
                    if opts.secrets_only && finding.severity != Severity::Critical {
                        continue;
                    }
                    record_finding(&mut pf, finding);
                }
            }
            Err(message) => {
                pf.screener_errors.insert(screener.name.clone(), message);
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
            if suppressed_patterns.iter().any(|p| p == &pattern.id) {
                continue;
            }
            let Some(mat) = pattern.regex.find(line) else {
                continue;
            };
            let mut severity = pattern.severity;
            // Docs (markdown, etc.) downgrade pathleak Warn to Info — the
            // README is the right place to *say* a home path like `/home/you`
            // exists; the wrong place is hardcoded source. Per-pattern `docs_downgrade`
            // opts out; secrets (Critical) never downgrade.
            if is_doc && severity == Severity::Warn && pattern.docs_downgrade {
                severity = Severity::Info;
            }
            if secrets_only && severity != Severity::Critical && !pattern.secrets_only_survives {
                continue;
            }
            let snippet = snippet_around(line, mat.start(), mat.end());
            let finding = Finding {
                project: project.to_string(),
                file: rel_str.clone(),
                line: line_no + 1,
                pattern_id: pattern.id.clone(),
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

/// How a screener's process output is translated into Findings.
#[derive(Debug, Clone, Copy)]
enum Adapter {
    /// orgmap-native NDJSON on stdout — one
    /// `{severity,file,line,rule,message}` object per line.
    Orgmap,
    /// gitleaks' JSON detect report.
    Gitleaks,
}

/// A resolved external screener (Lane B), built from `[[screen.screener]]`
/// config or the auto-injected gitleaks default.
struct Screener {
    name: String,
    command: String,
    args: Vec<String>,
    adapter: Adapter,
    optional: bool,
    /// Default tier for findings this screener doesn't tag with their own.
    severity: Severity,
    /// Expensive (full-history/entropy) — skipped on bulk org-wide runs
    /// unless `--deep`; always runs on a project-scoped scan.
    deep: bool,
}

impl Screener {
    /// The built-in gitleaks screener — auto-registered so orgmap's
    /// out-of-box behavior ("gitleaks runs if present") is unchanged. The
    /// gitleaks adapter bakes its own version-stable invocation, so `args`
    /// is empty here.
    fn builtin_gitleaks() -> Self {
        Self {
            name: "gitleaks".to_string(),
            command: "gitleaks".to_string(),
            args: Vec::new(),
            adapter: Adapter::Gitleaks,
            optional: true,
            severity: Severity::Critical,
            deep: true, // full git-history entropy walk — opt-in for bulk runs
        }
    }

    fn from_config(c: &ScreenScreener) -> Self {
        Self {
            name: c.name.clone(),
            command: c.command.clone(),
            args: c.args.clone(),
            adapter: match c.adapter.trim().to_ascii_lowercase().as_str() {
                "gitleaks" => Adapter::Gitleaks,
                _ => Adapter::Orgmap,
            },
            optional: c.optional,
            severity: severity_from_str(&c.severity),
            deep: c.deep,
        }
    }
}

/// Build the screener registry: configured entries first, then the built-in
/// gitleaks screener unless config already declares one named "gitleaks"
/// (lets an org redefine it). To disable gitleaks entirely, pass
/// `--skip-screener gitleaks`.
fn build_screeners(cfg: &ScreenConfig, org_root: &Path) -> Vec<Screener> {
    let mut out: Vec<Screener> = cfg.screeners.iter().map(Screener::from_config).collect();
    // Dir-discovered screeners: every executable in a screener_dir becomes an
    // orgmap-adapter screener named after its file stem. Explicit
    // [[screen.screener]] entries win on name collision; gitleaks injects last.
    for dir in &cfg.screener_dirs {
        for screener in discover_screener_dir(dir, org_root) {
            if !out.iter().any(|s| s.name == screener.name) {
                out.push(screener);
            }
        }
    }
    if !out.iter().any(|s| s.name == "gitleaks") {
        out.push(Screener::builtin_gitleaks());
    }
    out
}

/// Glob a directory for executable screener scripts (the `screener_dirs`
/// convenience). Each becomes an orgmap-adapter screener named after its file
/// stem, invoked as `script {project}` with cwd set to the project.
/// Non-executables (READMEs, etc.) and unreadable dirs are silently ignored.
fn discover_screener_dir(dir: &str, org_root: &Path) -> Vec<Screener> {
    let resolved = dir.replace("{org_root}", &org_root.display().to_string());
    let base = {
        let p = Path::new(&resolved);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            org_root.join(p)
        }
    };
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries.filter_map(|e| e.ok().map(|e| e.path())).collect();
    paths.sort(); // deterministic registration order
    let mut out = Vec::new();
    for p in paths {
        if !is_executable_file(&p) {
            continue;
        }
        let Some(name) = p.file_stem().and_then(OsStr::to_str) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        out.push(Screener {
            name: name.to_string(),
            command: p.display().to_string(),
            args: vec!["{project}".to_string()],
            adapter: Adapter::Orgmap,
            optional: false,
            severity: Severity::Warn,
            deep: false, // dir-discovered scripts are cheap; always run
        });
    }
    out
}

fn is_executable_file(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    match std::fs::metadata(p) {
        Ok(m) => m.is_file() && (m.permissions().mode() & 0o111 != 0),
        Err(_) => false,
    }
}

/// The screeners that will actually run: registry minus `--skip-screener`'d
/// names, minus optional screeners whose command can't be found. Shared by
/// `run` and `active_screeners` so the resolution rule lives in one place.
fn resolve_screeners(cfg: &ScreenConfig, org_root: &Path, opts: &ScreenOptions) -> Vec<Screener> {
    build_screeners(cfg, org_root)
        .into_iter()
        .filter(|s| !opts.skip_screeners.iter().any(|n| n == &s.name))
        // Deep screeners (gitleaks history walk) only run when scoped/--deep.
        .filter(|s| !s.deep || opts.run_deep)
        .filter(|s| command_resolves(&s.command, org_root) || !s.optional)
        .collect()
}

/// Names of the screeners that would run for this institution/options,
/// resolved without scanning any project. Powers `--list-screeners`.
pub fn active_screeners(institution: &Institution, opts: &ScreenOptions) -> Vec<String> {
    let cfg = institution_screen_config(institution);
    resolve_screeners(&cfg, &institution.root, opts)
        .into_iter()
        .map(|s| s.name)
        .collect()
}

/// One internal (built-in) pattern screener, for `--list-screeners` / `org
/// screeners` — so callers can SEE the full verification surface, not just the
/// external registry. `detail` is the regex (or matched literal) it scans for.
pub struct ScreenerInfo {
    pub name: String,
    pub severity: Severity,
    pub detail: String,
}

/// Enumerate the always-on internal pattern screeners: the baked-in secret +
/// sloppy patterns plus the config-derived pathleak patterns (personal paths +
/// handles). These run on every scan regardless of the external registry — this
/// is what makes them invisible without an explicit listing.
pub fn internal_screeners(institution: &Institution) -> Vec<ScreenerInfo> {
    let cfg = institution_screen_config(institution);
    let mut out: Vec<ScreenerInfo> = Vec::new();
    for p in build_static_patterns() {
        out.push(ScreenerInfo {
            name: p.id,
            severity: p.severity,
            detail: p.regex.as_str().to_string(),
        });
    }
    for p in build_dynamic_patterns(&cfg) {
        out.push(ScreenerInfo {
            name: p.id,
            severity: p.severity,
            detail: p.regex.as_str().to_string(),
        });
    }
    // Critical first, then Warn, then Info — most-severe surface up top.
    out.sort_by(|a, b| {
        (b.severity as u8)
            .cmp(&(a.severity as u8))
            .then_with(|| a.name.cmp(&b.name))
    });
    out
}

/// Whether a screener's command can be found. A bare name is looked up on
/// `$PATH`; a path (after `{org_root}` substitution) is checked as a file.
fn command_resolves(command: &str, org_root: &Path) -> bool {
    let cmd = command.replace("{org_root}", &org_root.display().to_string());
    if cmd.contains('/') {
        Path::new(&cmd).is_file()
    } else {
        which(&cmd).is_some()
    }
}

fn which(name: &str) -> Option<PathBuf> {
    // Cheap PATH walk so we don't shell out to `which` itself.
    let path = std::env::var_os("PATH")?;
    for entry in std::env::split_paths(&path) {
        let candidate = entry.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn substitute(s: &str, project_path: &Path, org_root: &Path) -> String {
    s.replace("{project}", &project_path.display().to_string())
        .replace("{org_root}", &org_root.display().to_string())
}

fn project_name_of(project_path: &Path) -> String {
    project_path
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_string()
}

/// Run one screener over a project and translate its output into Findings via
/// the screener's adapter.
fn run_screener(
    screener: &Screener,
    project_path: &Path,
    org_root: &Path,
) -> Result<Vec<Finding>, String> {
    match screener.adapter {
        Adapter::Gitleaks => run_gitleaks_adapter(screener, project_path),
        Adapter::Orgmap => run_orgmap_adapter(screener, project_path, org_root),
    }
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

/// Built-in adapter for gitleaks' JSON detect report. Writes to a temp file
/// rather than stdout — gitleaks mingles human-readable progress chatter on
/// stdout/stderr and the `--report-path -` contract has shifted between
/// versions, so the temp-file dance is the stable shape. Walks history by
/// default (rotated-but-committed keys are still leaks).
fn run_gitleaks_adapter(screener: &Screener, project_path: &Path) -> Result<Vec<Finding>, String> {
    let report_path = std::env::temp_dir().join(format!(
        "orgmap-{}-{}.json",
        screener.name,
        std::process::id()
    ));
    let output = Command::new(&screener.command)
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
        .map_err(|e| format!("{} spawn failed: {e}", screener.name))?;

    if !output.status.success() && !report_path.exists() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} errored: {}", screener.name, stderr.trim()));
    }

    let raw = std::fs::read_to_string(&report_path).map_err(|e| format!("read report: {e}"))?;
    let _ = std::fs::remove_file(&report_path);
    if raw.trim().is_empty() {
        return Ok(Vec::new());
    }
    let records: Vec<GitleaksRecord> =
        serde_json::from_str(&raw).map_err(|e| format!("parse report: {e}"))?;
    let project_name = project_name_of(project_path);
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
                pattern_id: format!("{}:{}", screener.name, r.rule_id),
                severity: screener.severity,
                snippet,
                source: FindingSource::Screener(screener.name.clone()),
            }
        })
        .collect())
}

/// One finding line of the orgmap-native screener protocol. A custom screener
/// prints NDJSON to stdout, one of these per line.
#[derive(Debug, serde::Deserialize)]
struct OrgmapFindingRecord {
    #[serde(default)]
    severity: String,
    #[serde(default)]
    file: String,
    #[serde(default)]
    line: usize,
    #[serde(default)]
    rule: String,
    #[serde(default)]
    message: String,
}

/// Built-in adapter for the orgmap-native protocol: run the org's command and
/// parse NDJSON findings from stdout. This is the seam that lets an org plug
/// in any language/taste-specific check without orgmap learning the language.
fn run_orgmap_adapter(
    screener: &Screener,
    project_path: &Path,
    org_root: &Path,
) -> Result<Vec<Finding>, String> {
    let cmd = substitute(&screener.command, project_path, org_root);
    let args: Vec<String> = screener
        .args
        .iter()
        .map(|a| substitute(a, project_path, org_root))
        .collect();
    let output = Command::new(&cmd)
        .args(&args)
        .current_dir(project_path)
        .output()
        .map_err(|e| format!("{} spawn failed: {e}", screener.name))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let code = output
            .status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "signal".to_string());
        return Err(format!("{} exited {}: {}", screener.name, code, stderr.trim()));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let project_name = project_name_of(project_path);
    let mut out = Vec::new();
    for (i, raw) in stdout.lines().enumerate() {
        let raw = raw.trim();
        if raw.is_empty() {
            continue;
        }
        let rec: OrgmapFindingRecord = serde_json::from_str(raw)
            .map_err(|e| format!("{} line {}: bad NDJSON: {e}", screener.name, i + 1))?;
        if rec.file.is_empty() {
            continue;
        }
        let severity = if rec.severity.is_empty() {
            screener.severity
        } else {
            severity_from_str(&rec.severity)
        };
        let pattern_id = if rec.rule.is_empty() {
            screener.name.clone()
        } else {
            format!("{}:{}", screener.name, rec.rule)
        };
        out.push(Finding {
            project: project_name.clone(),
            file: relative_or_self(project_path, &rec.file),
            line: rec.line,
            pattern_id,
            severity,
            snippet: rec.message,
            source: FindingSource::Screener(screener.name.clone()),
        });
    }
    Ok(out)
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
