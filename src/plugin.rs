//! Agent plugin discovery for the `org plug` report.
//!
//! Plugin carriers are org projects that expose Claude Code, Codex, or
//! compatible agent-harness manifests. The scanner deliberately treats root
//! manifests and bundled manifests as different surfaces: HSP, for example,
//! ships both a repository-level Codex plugin and the installable
//! `plugins/hsp/.codex-plugin` bundle. Reference and archive trees are skipped
//! so old examples do not masquerade as live org inventory.

use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Serialize;
use serde_json::Value;

use crate::institution::{GitState, Institution, Project};

#[derive(Debug, Clone, Serialize)]
pub struct PluginReport {
    pub projects: usize,
    pub surfaces: usize,
    pub claude: usize,
    pub codex: usize,
    pub invalid: usize,
    pub upstream: Option<PluginUpstreamSummary>,
    pub plugins: Vec<PluginSurface>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginSurface {
    pub project: String,
    pub workspace: String,
    pub host: PluginHost,
    pub manifest: PathBuf,
    pub root: PathBuf,
    pub name: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub skills: Option<PluginFeature>,
    pub mcp_servers: Option<PluginFeature>,
    pub lsp_servers: Option<PluginFeature>,
    pub hooks: Option<PluginFeature>,
    pub interface: bool,
    pub valid: bool,
    pub error: Option<String>,
    pub carrier_git: Option<GitState>,
    pub upstream: Option<PluginUpstream>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PluginHost {
    Claude,
    Codex,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginFeature {
    pub path: Option<String>,
    pub count: usize,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginUpstreamSummary {
    pub synced: usize,
    pub changed: usize,
    pub missing: usize,
    pub error: usize,
    pub dirty: usize,
    pub ahead: usize,
    pub behind: usize,
    pub no_upstream: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct PluginUpstream {
    pub status: PluginUpstreamStatus,
    pub repo: String,
    pub path: String,
    pub local_sha: Option<String>,
    pub remote_sha: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum PluginUpstreamStatus {
    Synced,
    Changed,
    Missing,
    Error,
}

pub fn plugin_report(institution: &Institution, verify_upstream: bool) -> PluginReport {
    let mut plugins = institution
        .projects
        .iter()
        .filter(|project| project.local && !project.blacklisted)
        .flat_map(|project| plugin_surfaces(project, &institution.gh_org, verify_upstream))
        .collect::<Vec<_>>();
    plugins.sort_by(|a, b| {
        a.project
            .cmp(&b.project)
            .then_with(|| host_rank(a.host).cmp(&host_rank(b.host)))
            .then_with(|| a.manifest.cmp(&b.manifest))
    });

    let projects = plugins
        .iter()
        .map(|plugin| plugin.project.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    let claude = plugins
        .iter()
        .filter(|plugin| plugin.host == PluginHost::Claude)
        .count();
    let codex = plugins
        .iter()
        .filter(|plugin| plugin.host == PluginHost::Codex)
        .count();
    let invalid = plugins.iter().filter(|plugin| !plugin.valid).count();
    let upstream = verify_upstream.then(|| upstream_summary(&plugins));

    PluginReport {
        projects,
        surfaces: plugins.len(),
        claude,
        codex,
        invalid,
        upstream,
        plugins,
    }
}

fn plugin_surfaces(project: &Project, org: &str, verify_upstream: bool) -> Vec<PluginSurface> {
    let Some(path) = &project.path else {
        return Vec::new();
    };
    find_plugin_manifests(path)
        .into_iter()
        .map(|(host, manifest)| plugin_surface(project, org, host, manifest, verify_upstream))
        .collect()
}

fn plugin_surface(
    project: &Project,
    org: &str,
    host: PluginHost,
    manifest: PathBuf,
    verify_upstream: bool,
) -> PluginSurface {
    let root = plugin_root(&manifest);
    let parsed = std::fs::read_to_string(&manifest)
        .map_err(|error| error.to_string())
        .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|error| error.to_string()));
    let upstream = verify_upstream.then(|| verify_manifest_upstream(org, project, &manifest));

    match parsed {
        Ok(value) => PluginSurface {
            project: project.name.clone(),
            workspace: project.workspace.clone(),
            host,
            manifest,
            root: root.clone(),
            name: string_field(&value, "name"),
            version: string_field(&value, "version"),
            description: string_field(&value, "description"),
            skills: feature_from_value(&root, value.get("skills"), count_skills),
            mcp_servers: feature_from_value(&root, value.get("mcpServers"), count_mapping_feature),
            lsp_servers: feature_from_value(&root, value.get("lspServers"), count_mapping_feature),
            hooks: feature_from_value(&root, value.get("hooks"), count_mapping_feature),
            interface: value.get("interface").is_some(),
            valid: true,
            error: None,
            carrier_git: project.git.clone(),
            upstream,
        },
        Err(error) => PluginSurface {
            project: project.name.clone(),
            workspace: project.workspace.clone(),
            host,
            manifest,
            root,
            name: None,
            version: None,
            description: None,
            skills: None,
            mcp_servers: None,
            lsp_servers: None,
            hooks: None,
            interface: false,
            valid: false,
            error: Some(error),
            carrier_git: project.git.clone(),
            upstream,
        },
    }
}

fn verify_manifest_upstream(org: &str, project: &Project, manifest: &Path) -> PluginUpstream {
    let repo = project
        .path
        .as_ref()
        .and_then(|path| github_remote_repo(path))
        .unwrap_or_else(|| format!("{org}/{}", project.name));
    let path = project
        .path
        .as_ref()
        .and_then(|root| manifest.strip_prefix(root).ok())
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|| manifest.display().to_string());
    let local_sha = git_hash_object(manifest);
    let remote = gh_content_sha(&repo, &path);

    match remote {
        Ok(remote_sha) => {
            let status = if local_sha.as_deref() == Some(remote_sha.as_str()) {
                PluginUpstreamStatus::Synced
            } else {
                PluginUpstreamStatus::Changed
            };
            PluginUpstream {
                status,
                repo,
                path,
                local_sha,
                remote_sha: Some(remote_sha),
                message: None,
            }
        }
        Err(error) if error.is_not_found => PluginUpstream {
            status: PluginUpstreamStatus::Missing,
            repo,
            path,
            local_sha,
            remote_sha: None,
            message: Some(error.message),
        },
        Err(error) => PluginUpstream {
            status: PluginUpstreamStatus::Error,
            repo,
            path,
            local_sha,
            remote_sha: None,
            message: Some(error.message),
        },
    }
}

fn upstream_summary(plugins: &[PluginSurface]) -> PluginUpstreamSummary {
    let carriers = plugins
        .iter()
        .map(|plugin| plugin.project.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let carrier_git = carriers
        .iter()
        .filter_map(|project| {
            plugins
                .iter()
                .find(|plugin| plugin.project == **project)
                .and_then(|plugin| plugin.carrier_git.as_ref())
        })
        .collect::<Vec<_>>();

    PluginUpstreamSummary {
        synced: plugins
            .iter()
            .filter(|plugin| {
                plugin
                    .upstream
                    .as_ref()
                    .is_some_and(|upstream| upstream.status == PluginUpstreamStatus::Synced)
            })
            .count(),
        changed: plugins
            .iter()
            .filter(|plugin| {
                plugin
                    .upstream
                    .as_ref()
                    .is_some_and(|upstream| upstream.status == PluginUpstreamStatus::Changed)
            })
            .count(),
        missing: plugins
            .iter()
            .filter(|plugin| {
                plugin
                    .upstream
                    .as_ref()
                    .is_some_and(|upstream| upstream.status == PluginUpstreamStatus::Missing)
            })
            .count(),
        error: plugins
            .iter()
            .filter(|plugin| {
                plugin
                    .upstream
                    .as_ref()
                    .is_some_and(|upstream| upstream.status == PluginUpstreamStatus::Error)
            })
            .count(),
        dirty: carrier_git.iter().filter(|git| git.dirty > 0).count(),
        ahead: carrier_git
            .iter()
            .filter(|git| git.ahead.unwrap_or(0) > 0)
            .count(),
        behind: carrier_git
            .iter()
            .filter(|git| git.behind.unwrap_or(0) > 0)
            .count(),
        no_upstream: carrier_git.iter().filter(|git| !git.has_upstream).count(),
    }
}

#[derive(Debug)]
struct GhContentError {
    message: String,
    is_not_found: bool,
}

fn gh_content_sha(repo: &str, path: &str) -> Result<String, GhContentError> {
    let output = Command::new("gh")
        .args(["api", &format!("repos/{repo}/contents/{path}"), "--jq", ".sha"])
        .output()
        .map_err(|error| GhContentError {
            message: error.to_string(),
            is_not_found: false,
        })?;
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let message = if stderr.is_empty() { stdout } else { stderr };
    let is_not_found = message.contains("HTTP 404") || message.contains("Not Found");
    Err(GhContentError {
        message,
        is_not_found,
    })
}

fn git_hash_object(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("hash-object")
        .arg(path)
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn github_remote_repo(path: &Path) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_github_repo(String::from_utf8_lossy(&output.stdout).trim())
}

fn parse_github_repo(remote: &str) -> Option<String> {
    let remote = remote.trim().trim_end_matches(".git");
    if let Some(rest) = remote.strip_prefix("git@github.com:") {
        return Some(rest.to_string());
    }
    if let Some(rest) = remote.strip_prefix("ssh://git@github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = remote.strip_prefix("https://github.com/") {
        return Some(rest.to_string());
    }
    if let Some(rest) = remote.strip_prefix("http://github.com/") {
        return Some(rest.to_string());
    }
    None
}

fn find_plugin_manifests(project: &Path) -> Vec<(PluginHost, PathBuf)> {
    let mut found = Vec::new();
    push_manifest(project, &mut found);

    let plugin_dir = project.join("plugins");
    if let Ok(entries) = std::fs::read_dir(plugin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !ignored_dir(&path) {
                push_manifest(&path, &mut found);
            }
        }
    }
    found
}

fn plugin_root(manifest: &Path) -> PathBuf {
    let plugin_dir = manifest.parent().unwrap_or(manifest);
    if plugin_dir
        .file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| name == ".claude-plugin" || name == ".codex-plugin")
    {
        return plugin_dir
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| plugin_dir.to_path_buf());
    }
    plugin_dir.to_path_buf()
}

fn push_manifest(path: &Path, found: &mut Vec<(PluginHost, PathBuf)>) {
    for (dir, host) in [
        (".claude-plugin", PluginHost::Claude),
        (".codex-plugin", PluginHost::Codex),
    ] {
        let manifest = path.join(dir).join("plugin.json");
        if manifest.is_file() {
            found.push((host, manifest));
        }
    }
}

fn ignored_dir(path: &Path) -> bool {
    path.file_name()
        .and_then(OsStr::to_str)
        .is_some_and(|name| {
            matches!(
                name,
                ".git" | "target" | "node_modules" | "tmp" | ".archive" | "references"
            )
        })
}

fn feature_from_value(
    root: &Path,
    value: Option<&Value>,
    counter: fn(&Path, &Value) -> usize,
) -> Option<PluginFeature> {
    let value = value?;
    let path = value.as_str().map(str::to_string);
    let exists = path
        .as_ref()
        .map(|path| root.join(path).exists())
        .unwrap_or(true);
    Some(PluginFeature {
        path,
        count: counter(root, value),
        exists,
    })
}

fn count_mapping_feature(_root: &Path, value: &Value) -> usize {
    match value {
        Value::Object(map) => map.len(),
        Value::String(_) => 1,
        Value::Array(list) => list.len(),
        _ => 0,
    }
}

fn count_skills(root: &Path, value: &Value) -> usize {
    let Some(path) = value.as_str() else {
        return count_mapping_feature(root, value);
    };
    let skill_root = root.join(path);
    let Ok(entries) = std::fs::read_dir(skill_root) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .count()
}

fn string_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn host_rank(host: PluginHost) -> u8 {
    match host {
        PluginHost::Claude => 0,
        PluginHost::Codex => 1,
    }
}
