//! Institution graph loading for the `org` command.
//!
//! The marker protocol answers "where am I?" at directory scope. This module
//! loads the full root `orgmap.toml` and turns it into the first operational
//! institution view: workspaces, known projects, local checkout state, and the
//! short familiarization report agents need before touching code.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::ORGMAP_FILE;

#[derive(Debug, Clone, Deserialize)]
pub struct OrgConfig {
    pub scan: ScanConfig,
    #[serde(default)]
    pub workspaces: BTreeMap<String, WorkspaceConfig>,
    #[serde(default)]
    pub sections: BTreeMap<String, Vec<String>>,
    #[serde(default)]
    pub overrides: BTreeMap<String, ProjectOverride>,
    #[serde(default)]
    pub screen: ScreenConfig,
}

/// `[screen]` block in orgmap.toml — anti-sloppy normalizer config. Drives
/// `org screen`. Pattern set itself is baked into screen.rs; this struct
/// supplies the user-tunable surface (identity strings, allowlists).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScreenConfig {
    /// Absolute path prefixes that count as personal identity leaks when
    /// they appear in tracked text. Default: `["/home/<user>", "/Users/<user>"]`
    /// derived from $HOME if empty.
    #[serde(default)]
    pub personal_paths: Vec<String>,
    /// Usernames, handles, and email addresses that should not appear in
    /// public-shipping text. Free-form substrings.
    #[serde(default)]
    pub personal_handles: Vec<String>,
    /// Per-project allowlist. Keys are project names (matching `orgmap`
    /// project rows). Values describe what to ignore inside that repo.
    #[serde(default)]
    pub allow: BTreeMap<String, ScreenAllow>,
    /// Org-wide allowlist applied to every project before per-project
    /// allow lookups.
    #[serde(default)]
    pub global_allow: ScreenAllow,
    /// Lane A — org-defined declarative screening patterns (`[[screen.pattern]]`).
    /// Pure regex, appended to orgmap's baked universal set. orgmap ships NONE
    /// of these; every entry is the adopting org's own invariant. See
    /// docs/screen-extensibility.md. (TOML key is the singular array-of-tables
    /// `[[screen.pattern]]`; the field is plural.)
    #[serde(default, rename = "pattern")]
    pub patterns: Vec<ScreenPattern>,
    /// Lane B — org-registered external screeners (`[[screen.screener]]`).
    /// Each is an external command orgmap runs per project and whose findings
    /// it ingests via `adapter`. The built-in `gitleaks` screener is
    /// auto-registered unless a screener named "gitleaks" appears here. orgmap
    /// ships no org-specific screeners; language/taste rules live as the org's
    /// own scripts, never in orgmap core.
    #[serde(default, rename = "screener")]
    pub screeners: Vec<ScreenScreener>,
}

/// One org-defined declarative pattern (Lane A). Regex-only and
/// language-agnostic — the safe extension lane. `severity` is a string
/// (`critical`|`warn`|`info`) parsed by the screen engine.
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenPattern {
    /// Stable handle used by allowlists and surfaced on report rows.
    pub id: String,
    /// `critical` | `warn` | `info`. Defaults to `warn`.
    #[serde(default = "screen_default_warn")]
    pub severity: String,
    /// The regex (RE2 syntax, `regex` crate).
    pub regex: String,
    /// Downgrade a `warn` match to `info` inside doc files (md/txt/rst).
    /// Defaults true, mirroring the universal pathleak behavior.
    #[serde(default = "screen_default_true")]
    pub docs_downgrade: bool,
    /// Survive the `--secrets-only` gate even when not `critical`. Default false.
    #[serde(default)]
    pub secrets_only: bool,
}

/// One org-registered external screener (Lane B). orgmap runs
/// `command args…` per project (substituting `{project}` / `{org_root}`)
/// and ingests its output via `adapter`. This is how an org plugs in
/// language- or taste-specific checks without a line in orgmap core.
#[derive(Debug, Clone, Deserialize)]
pub struct ScreenScreener {
    /// Display name + provenance tag stamped on every finding it produces.
    pub name: String,
    /// Executable — resolved on `$PATH` if bare, else a path (supports
    /// the `{org_root}` placeholder).
    pub command: String,
    /// Arguments. `{project}` → absolute project path, `{org_root}` → org root.
    #[serde(default)]
    pub args: Vec<String>,
    /// Output translator: `orgmap` (native NDJSON on stdout) or `gitleaks`
    /// (its JSON report). Defaults to `orgmap`.
    #[serde(default = "screen_default_adapter")]
    pub adapter: String,
    /// Missing binary → skip silently (true) or record an error (false).
    /// Defaults true, matching the gitleaks-optional behavior.
    #[serde(default = "screen_default_true")]
    pub optional: bool,
    /// Default tier for findings that don't carry their own `severity`.
    /// Defaults to `critical`.
    #[serde(default = "screen_default_critical")]
    pub severity: String,
}

fn screen_default_true() -> bool {
    true
}
fn screen_default_warn() -> String {
    "warn".to_string()
}
fn screen_default_critical() -> String {
    "critical".to_string()
}
fn screen_default_adapter() -> String {
    "orgmap".to_string()
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScreenAllow {
    /// Repo-relative path prefixes whose findings are suppressed entirely.
    #[serde(default)]
    pub files: Vec<String>,
    /// File extensions (with or without leading dot) whose pathleak
    /// findings are downgraded to advisory — markdown docs reference user
    /// paths legitimately.
    #[serde(default)]
    pub doc_extensions: Vec<String>,
    /// Pattern IDs to suppress entirely for this scope.
    #[serde(default)]
    pub patterns: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ScanConfig {
    pub gh_org: String,
    #[serde(default)]
    pub roots: Vec<String>,
    #[serde(default)]
    pub blacklist: Vec<String>,
    #[serde(default)]
    pub default_section: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct WorkspaceConfig {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub emoji: Option<String>,
    #[serde(default)]
    pub subtitle: Option<String>,
    #[serde(default)]
    pub preamble: Option<String>,
    #[serde(default)]
    pub order: Option<i64>,
    #[serde(default)]
    pub internal: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProjectOverride {
    #[serde(default)]
    pub display_name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub tagline: Option<String>,
    #[serde(default)]
    pub stage: Option<toml::Value>,
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Institution {
    pub config_path: PathBuf,
    pub root: PathBuf,
    pub gh_org: String,
    pub workspaces: Vec<Workspace>,
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Copy)]
pub struct LoadOptions {
    pub git: bool,
}

impl LoadOptions {
    pub const MAP_ONLY: Self = Self { git: false };
    pub const WITH_GIT: Self = Self { git: true };
}

#[derive(Debug, Clone, Serialize)]
pub struct Workspace {
    pub key: String,
    pub display_name: String,
    pub emoji: String,
    pub subtitle: Option<String>,
    pub preamble: Option<String>,
    pub order: i64,
    pub internal: bool,
    pub root: Option<PathBuf>,
    pub projects: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Project {
    pub name: String,
    pub display_name: String,
    pub workspace: String,
    pub section: String,
    pub path: Option<PathBuf>,
    pub stage: Option<String>,
    pub description: Option<String>,
    pub tagline: Option<String>,
    pub local: bool,
    pub blacklisted: bool,
    pub git: Option<GitState>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GitState {
    pub branch: String,
    pub dirty: usize,
    pub ahead: Option<usize>,
    pub behind: Option<usize>,
    pub has_upstream: bool,
    pub age: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkReport {
    pub config_path: PathBuf,
    pub root: PathBuf,
    pub gh_org: String,
    pub workspaces: usize,
    pub projects: usize,
    pub local_projects: usize,
    pub dirty: Vec<Project>,
    pub ahead: Vec<Project>,
    pub no_upstream: Vec<Project>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IntroReport {
    pub config_path: PathBuf,
    pub root: PathBuf,
    pub gh_org: String,
    pub workspaces: Vec<WorkspaceIntro>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WorkspaceIntro {
    pub key: String,
    pub display_name: String,
    pub emoji: String,
    pub subtitle: Option<String>,
    pub preamble: Option<String>,
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OrgListing {
    pub name: String,
    pub root: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub source: String,
    pub default: bool,
    pub active: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct Registry {
    #[serde(default)]
    default: Option<String>,
    #[serde(default)]
    institutions: BTreeMap<String, RegistryInstitution>,
}

#[derive(Debug, Clone, Deserialize)]
struct RegistryInstitution {
    #[serde(default)]
    root: Option<String>,
    #[serde(default)]
    config: Option<String>,
}

#[derive(Debug, Clone)]
struct LocalProject {
    name: String,
    workspace: String,
    path: PathBuf,
    git_meta: Option<toml::Value>,
}

#[derive(Debug, Clone)]
struct SectionEntry {
    name: String,
    stage: Option<String>,
}

pub fn discover_config_path(location: &Path) -> Option<PathBuf> {
    if let Some(path) = env_config_path() {
        return Some(path);
    }

    let start = normalize_location(location);
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(ORGMAP_FILE);
        if is_full_orgmap(&candidate) {
            return Some(canonical_or_self(candidate));
        }
    }

    registry_default_config_path()
}

pub fn load_institution(config_path: &Path) -> Result<Institution, Box<dyn std::error::Error>> {
    load_institution_with_options(config_path, LoadOptions::WITH_GIT)
}

pub fn load_institution_with_options(
    config_path: &Path,
    options: LoadOptions,
) -> Result<Institution, Box<dyn std::error::Error>> {
    let config_path = canonical_or_self(expand_user_path(config_path));
    let text = std::fs::read_to_string(&config_path)?;
    let config: OrgConfig = toml::from_str(&text)?;
    let root = config_path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let local = scan_local_projects(&config);
    let projects = build_projects(&config, &local, options);
    let workspaces = build_workspaces(&config, &projects);

    Ok(Institution {
        config_path,
        root,
        gh_org: config.scan.gh_org,
        workspaces,
        projects,
    })
}

pub fn load_current_institution(
    config: Option<&Path>,
    location: &Path,
) -> Result<Institution, Box<dyn std::error::Error>> {
    let config_path = match config {
        Some(path) => expand_user_path(path),
        None => discover_config_path(location).ok_or("no root orgmap.toml found")?,
    };
    load_institution(&config_path)
}

pub fn load_current_institution_with_options(
    config: Option<&Path>,
    location: &Path,
    options: LoadOptions,
) -> Result<Institution, Box<dyn std::error::Error>> {
    let config_path = match config {
        Some(path) => expand_user_path(path),
        None => discover_config_path(location).ok_or("no root orgmap.toml found")?,
    };
    load_institution_with_options(&config_path, options)
}

pub fn work_report(institution: &Institution) -> WorkReport {
    let mut dirty = Vec::new();
    let mut ahead = Vec::new();
    let mut no_upstream = Vec::new();

    for project in institution.projects.iter().filter(|project| project.local) {
        let Some(git) = &project.git else {
            continue;
        };
        if git.dirty > 0 {
            dirty.push(project.clone());
        }
        if git.ahead.unwrap_or(0) > 0 {
            ahead.push(project.clone());
        }
        if !git.has_upstream {
            no_upstream.push(project.clone());
        }
    }

    WorkReport {
        config_path: institution.config_path.clone(),
        root: institution.root.clone(),
        gh_org: institution.gh_org.clone(),
        workspaces: institution.workspaces.len(),
        projects: institution.projects.len(),
        local_projects: institution
            .projects
            .iter()
            .filter(|project| project.local)
            .count(),
        dirty,
        ahead,
        no_upstream,
    }
}

pub fn intro_report(institution: &Institution) -> IntroReport {
    let workspaces = institution
        .workspaces
        .iter()
        .filter(|workspace| !workspace.internal)
        .map(|workspace| {
            let mut projects = institution
                .projects
                .iter()
                .filter(|project| project.section == workspace.key && !project.blacklisted)
                .cloned()
                .collect::<Vec<_>>();
            projects.sort_by(|a, b| {
                stage_rank(a.stage.as_deref())
                    .cmp(&stage_rank(b.stage.as_deref()))
                    .then_with(|| a.name.cmp(&b.name))
            });
            WorkspaceIntro {
                key: workspace.key.clone(),
                display_name: workspace.display_name.clone(),
                emoji: workspace.emoji.clone(),
                subtitle: workspace.subtitle.clone(),
                preamble: workspace.preamble.clone(),
                projects,
            }
        })
        .collect();

    IntroReport {
        config_path: institution.config_path.clone(),
        root: institution.root.clone(),
        gh_org: institution.gh_org.clone(),
        workspaces,
    }
}

pub fn config_dir_path() -> Option<PathBuf> {
    xdg_config_home().map(|path| path.join("orgmap").join("config"))
}

pub fn configured_orgs(location: &Path) -> Vec<OrgListing> {
    let active_config = discover_config_path(location);
    let current = normalize_location(location);
    let mut orgs = Vec::new();

    if let Some(org) = env_org_listing() {
        orgs.push(org);
    }
    orgs.extend(config_dir_orgs());
    orgs.extend(legacy_registry_orgs());

    for org in &mut orgs {
        org.active = org_is_active(org, &current, active_config.as_deref());
    }

    let has_default = orgs.iter().any(|org| org.default);
    if !has_default && orgs.len() == 1 {
        if let Some(org) = orgs.first_mut() {
            org.default = true;
        }
    }

    orgs.sort_by(|a, b| {
        b.default
            .cmp(&a.default)
            .then_with(|| b.active.cmp(&a.active))
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.source.cmp(&b.source))
    });
    orgs
}

fn build_projects(config: &OrgConfig, local: &[LocalProject], options: LoadOptions) -> Vec<Project> {
    let section_entries = section_entries(config);
    let mut rows = BTreeMap::<String, Project>::new();
    let mut section_names = BTreeSet::<String>::new();

    for (section, entries) in &section_entries {
        for entry in entries {
            section_names.insert(entry.name.clone());
            let local_project = local.iter().find(|project| project.name == entry.name);
            rows.insert(
                entry.name.clone(),
                project_from_sources(config, section, Some(entry), local_project, options),
            );
        }
    }

    for local_project in local {
        if section_names.contains(&local_project.name) {
            continue;
        }
        let section = override_section(config, &local_project.name)
            .unwrap_or_else(|| local_project.workspace.clone());
        rows.insert(
            local_project.name.clone(),
            project_from_sources(config, &section, None, Some(local_project), options),
        );
    }

    let mut projects = rows.into_values().collect::<Vec<_>>();
    projects.sort_by(|a, b| {
        a.section
            .cmp(&b.section)
            .then_with(|| stage_rank(a.stage.as_deref()).cmp(&stage_rank(b.stage.as_deref())))
            .then_with(|| a.name.cmp(&b.name))
    });
    projects
}

fn project_from_sources(
    config: &OrgConfig,
    section: &str,
    section_entry: Option<&SectionEntry>,
    local: Option<&LocalProject>,
    options: LoadOptions,
) -> Project {
    let name = section_entry
        .map(|entry| entry.name.as_str())
        .or_else(|| local.map(|project| project.name.as_str()))
        .unwrap_or_default();
    let override_cfg = config.overrides.get(name);
    let git_meta = local.and_then(|project| project.git_meta.as_ref());
    let section = override_cfg
        .and_then(|cfg| cfg.section.clone())
        .unwrap_or_else(|| section.to_string());
    let stage = override_cfg
        .and_then(|cfg| cfg.stage.as_ref())
        .and_then(stage_from_value)
        .or_else(|| section_entry.and_then(|entry| entry.stage.clone()))
        .or_else(|| {
            git_meta
                .and_then(|meta| meta.get("stage"))
                .and_then(stage_from_value)
        });
    let description = override_cfg
        .and_then(|cfg| cfg.description.clone())
        .or_else(|| git_meta.and_then(|meta| string_field(meta, "description")));

    Project {
        name: name.to_string(),
        display_name: override_cfg
            .and_then(|cfg| cfg.display_name.clone())
            .unwrap_or_else(|| name.to_string()),
        workspace: local
            .map(|project| project.workspace.clone())
            .unwrap_or_else(|| "upstream-only".to_string()),
        section,
        path: local.map(|project| project.path.clone()),
        stage,
        description,
        tagline: override_cfg.and_then(|cfg| cfg.tagline.clone()),
        local: local.is_some(),
        blacklisted: config.scan.blacklist.iter().any(|item| item == name),
        git: local
            .filter(|_| options.git)
            .map(|project| git_state(&project.path)),
    }
}

fn build_workspaces(config: &OrgConfig, projects: &[Project]) -> Vec<Workspace> {
    let root_by_workspace = config
        .scan
        .roots
        .iter()
        .map(|root| {
            let path = expand_user(root);
            let key = path
                .file_name()
                .and_then(OsStr::to_str)
                .unwrap_or_default()
                .to_string();
            (key, canonical_or_self(path))
        })
        .collect::<HashMap<_, _>>();

    let mut workspaces = config
        .workspaces
        .iter()
        .map(|(key, meta)| Workspace {
            key: key.clone(),
            display_name: meta.display_name.clone().unwrap_or_else(|| key.clone()),
            emoji: meta.emoji.clone().unwrap_or_default(),
            subtitle: meta.subtitle.clone(),
            preamble: meta.preamble.clone(),
            order: workspace_order(config, key),
            internal: meta.internal,
            root: root_by_workspace.get(key).cloned(),
            projects: projects
                .iter()
                .filter(|project| project.section == *key)
                .count(),
        })
        .collect::<Vec<_>>();
    workspaces.sort_by(|a, b| a.order.cmp(&b.order).then_with(|| a.key.cmp(&b.key)));
    workspaces
}

fn scan_local_projects(config: &OrgConfig) -> Vec<LocalProject> {
    let mut seen = BTreeSet::new();
    let mut projects = Vec::new();

    for root in &config.scan.roots {
        let root = expand_user(root);
        let workspace = root
            .file_name()
            .and_then(OsStr::to_str)
            .unwrap_or_default()
            .to_string();
        let Ok(entries) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() || !path.join(".git").exists() {
                continue;
            }
            let Some(name) = path.file_name().and_then(OsStr::to_str).map(str::to_string) else {
                continue;
            };
            if !seen.insert(name.clone()) {
                continue;
            }
            projects.push(LocalProject {
                name,
                workspace: workspace.clone(),
                git_meta: load_git_meta(&path),
                path: canonical_or_self(path),
            });
        }
    }

    projects
}

fn load_git_meta(path: &Path) -> Option<toml::Value> {
    let text = std::fs::read_to_string(path.join(".git-meta")).ok()?;
    toml::from_str(&text).ok()
}

fn section_entries(config: &OrgConfig) -> BTreeMap<String, Vec<SectionEntry>> {
    config
        .sections
        .iter()
        .map(|(section, entries)| {
            (
                section.clone(),
                entries
                    .iter()
                    .map(|entry| parse_section_entry(entry))
                    .collect(),
            )
        })
        .collect()
}

fn parse_section_entry(entry: &str) -> SectionEntry {
    let Some((prefix, rest)) = entry.split_once(':') else {
        return SectionEntry {
            name: entry.to_string(),
            stage: None,
        };
    };
    if prefix == "0" {
        return SectionEntry {
            name: rest.to_string(),
            stage: None,
        };
    }
    let stage = normalize_stage(prefix);
    if is_known_stage(stage.as_deref()) {
        SectionEntry {
            name: rest.to_string(),
            stage,
        }
    } else {
        SectionEntry {
            name: entry.to_string(),
            stage: None,
        }
    }
}

fn normalize_stage(stage: &str) -> Option<String> {
    match stage.trim() {
        "" | "0" => None,
        "1" | "research" => Some("research".to_string()),
        "2" | "beta" => Some("beta".to_string()),
        "3" | "certified" => Some("certified".to_string()),
        "-1" | "hazard-low" => Some("hazard-low".to_string()),
        "-2" | "hazard-high" => Some("hazard-high".to_string()),
        "archived" => Some("archived".to_string()),
        other => Some(other.to_string()),
    }
}

fn stage_from_value(value: &toml::Value) -> Option<String> {
    match value {
        toml::Value::String(text) => normalize_stage(text),
        toml::Value::Integer(value) => normalize_stage(&value.to_string()),
        _ => None,
    }
}

fn is_known_stage(stage: Option<&str>) -> bool {
    matches!(
        stage,
        Some("research" | "beta" | "certified" | "hazard-low" | "hazard-high" | "archived")
    )
}

fn stage_rank(stage: Option<&str>) -> u8 {
    match stage {
        Some("certified") => 0,
        Some("beta") => 1,
        Some("research") => 2,
        Some("hazard-low") => 3,
        Some("hazard-high") => 4,
        Some("archived") => 5,
        _ => 6,
    }
}

fn workspace_order(config: &OrgConfig, key: &str) -> i64 {
    if let Some(order) = config
        .workspaces
        .get(key)
        .and_then(|workspace| workspace.order)
    {
        return order;
    }
    config
        .scan
        .roots
        .iter()
        .position(|root| {
            expand_user(root)
                .file_name()
                .and_then(OsStr::to_str)
                .is_some_and(|name| name == key)
        })
        .map(|index| index as i64)
        .unwrap_or(999)
}

fn override_section(config: &OrgConfig, name: &str) -> Option<String> {
    config
        .overrides
        .get(name)
        .and_then(|override_cfg| override_cfg.section.clone())
}

fn string_field(value: &toml::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(toml::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn git_state(path: &Path) -> GitState {
    let branch = git_output(path, &["branch", "--show-current"])
        .filter(|branch| !branch.is_empty())
        .unwrap_or_else(|| "HEAD".to_string());
    let dirty = git_output(path, &["status", "--short"])
        .map(|text| text.lines().count())
        .unwrap_or(0);
    let counts = git_output(
        path,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
    )
    .and_then(|text| {
        let mut parts = text.split_whitespace();
        let behind = parts.next()?.parse::<usize>().ok()?;
        let ahead = parts.next()?.parse::<usize>().ok()?;
        Some((behind, ahead))
    });

    GitState {
        branch,
        dirty,
        ahead: counts.map(|(_, ahead)| ahead),
        behind: counts.map(|(behind, _)| behind),
        has_upstream: counts.is_some(),
        age: git_output(path, &["log", "-1", "--format=%cr"]).filter(|age| !age.is_empty()),
    }
}

fn git_output(path: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn is_full_orgmap(path: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = text.parse::<toml::Value>() else {
        return false;
    };
    value.get("scan").is_some() && value.get("workspaces").is_some()
}

fn env_config_path() -> Option<PathBuf> {
    let path = std::env::var("ORGMAP_CONFIG")
        .ok()
        .map(|raw| expand_user(raw.trim()))
        .or_else(|| {
            std::env::var("ORGMAP_ROOT")
                .ok()
                .map(|raw| expand_user(raw.trim()).join(ORGMAP_FILE))
        })?;
    is_full_orgmap(&path).then(|| canonical_or_self(path))
}

fn registry_default_config_path() -> Option<PathBuf> {
    configured_default_config_path().filter(|path| is_full_orgmap(path))
}

fn configured_default_config_path() -> Option<PathBuf> {
    config_dir_orgs()
        .into_iter()
        .find(|org| org.default)
        .and_then(|org| org.config_path)
        .or_else(|| {
            let orgs = config_dir_orgs();
            (orgs.len() == 1)
                .then(|| orgs.into_iter().next()?.config_path)
                .flatten()
        })
        .or_else(legacy_registry_default_config_path)
}

fn env_org_listing() -> Option<OrgListing> {
    let config = std::env::var("ORGMAP_CONFIG")
        .ok()
        .map(|raw| expand_user(raw.trim()));
    let root = std::env::var("ORGMAP_ROOT")
        .ok()
        .map(|raw| expand_user(raw.trim()));
    if config.is_none() && root.is_none() {
        return None;
    }
    let config = config.or_else(|| root.as_ref().map(|path| path.join(ORGMAP_FILE)));
    let root = root
        .or_else(|| {
            config
                .as_ref()
                .and_then(|path| path.parent().map(Path::to_path_buf))
        })
        .map(canonical_or_self);
    let config = config.map(canonical_or_self);
    let name = std::env::var("ORGMAP_NAME")
        .ok()
        .map(|name| name.trim().to_string())
        .filter(|name| !name.is_empty())
        .or_else(|| basename(root.as_deref()))
        .or_else(|| basename(config.as_ref().and_then(|path| path.parent())))
        .unwrap_or_else(|| "env".to_string());

    Some(OrgListing {
        name,
        root,
        config_path: config,
        source: "env".to_string(),
        default: true,
        active: false,
    })
}

fn config_dir_orgs() -> Vec<OrgListing> {
    let Some(dir) = config_dir_path() else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut orgs = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension() == Some(OsStr::new("toml"))).then_some(path)
        })
        .filter_map(|path| config_file_org(&path))
        .collect::<Vec<_>>();
    orgs.sort_by(|a, b| a.name.cmp(&b.name).then_with(|| a.source.cmp(&b.source)));
    orgs
}

fn config_file_org(path: &Path) -> Option<OrgListing> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return None;
    };
    let value = text.parse::<toml::Value>().ok()?;
    let table = value.get("org").unwrap_or(&value);
    let root = string_field(table, "root").map(|path| canonical_or_self(expand_user(&path)));
    let config = string_field(table, "config")
        .map(|path| canonical_or_self(expand_user(&path)))
        .or_else(|| root.as_ref().map(|path| path.join(ORGMAP_FILE)));
    let name = string_field(table, "name")
        .or_else(|| path.file_stem().and_then(OsStr::to_str).map(str::to_string))?;
    let default = table
        .get("default")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);

    Some(OrgListing {
        name,
        root,
        config_path: config,
        source: format!(
            "config/{}",
            path.file_name()
                .and_then(OsStr::to_str)
                .unwrap_or("unknown.toml")
        ),
        default,
        active: false,
    })
}

fn legacy_registry_orgs() -> Vec<OrgListing> {
    let Some(path) = legacy_registry_path() else {
        return Vec::new();
    };
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(registry) = toml::from_str::<Registry>(&text) else {
        return Vec::new();
    };
    let default = registry.default;
    registry
        .institutions
        .into_iter()
        .map(|(name, institution)| {
            let root = institution
                .root
                .as_ref()
                .map(|root| canonical_or_self(expand_user(root)));
            let config = institution
                .config
                .as_ref()
                .map(|config| canonical_or_self(expand_user(config)))
                .or_else(|| root.as_ref().map(|root| root.join(ORGMAP_FILE)));
            OrgListing {
                default: default.as_ref() == Some(&name),
                name,
                root,
                config_path: config,
                source: "config.toml".to_string(),
                active: false,
            }
        })
        .collect()
}

fn legacy_registry_default_config_path() -> Option<PathBuf> {
    legacy_registry_orgs()
        .into_iter()
        .find(|org| org.default)
        .and_then(|org| org.config_path)
}

fn legacy_registry_path() -> Option<PathBuf> {
    xdg_config_home().map(|path| path.join("orgmap").join("config.toml"))
}

fn org_is_active(org: &OrgListing, current: &Path, active_config: Option<&Path>) -> bool {
    if let Some(config) = &org.config_path {
        if active_config.is_some_and(|active| active == config) {
            return true;
        }
    }
    org.root
        .as_deref()
        .is_some_and(|root| current.starts_with(root))
}

fn basename(path: Option<&Path>) -> Option<String> {
    path.and_then(Path::file_name)
        .and_then(OsStr::to_str)
        .map(str::to_string)
        .filter(|value| !value.is_empty())
}

fn xdg_config_home() -> Option<PathBuf> {
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|home| home.join(".config")))
}

fn expand_user_path(path: &Path) -> PathBuf {
    if path == Path::new("~") {
        return home_dir().unwrap_or_else(|| path.to_path_buf());
    }
    let Some(text) = path.to_str() else {
        return path.to_path_buf();
    };
    expand_user(text)
}

fn expand_user(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = home_dir() {
            return home.join(rest);
        }
    }
    PathBuf::from(path)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn normalize_location(path: &Path) -> PathBuf {
    let path = expand_user_path(path);
    let absolute = if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
    let canonical = canonical_or_self(absolute);
    if canonical.is_file() {
        canonical
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(canonical)
    } else {
        canonical
    }
}

fn canonical_or_self(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}
