//! Organization/workgroup marker protocol for Holo-Q projects.
//!
//! A workgroup is a directory-scope identity mark. It is not a git repo, an
//! activity state, or a build root. Tools discover it by walking upward from a
//! path and reading the nearest `orgmap.toml`, `workgroup.toml`, or
//! `.hsp/workgroup.toml`.
//!
//! Standard file shape:
//!
//! ```toml
//! [workgroup]
//! name = "repo-os"
//! level = "domain"          # umbrella | domain | project | custom string
//! icon = "\U000f0493"       # optional visual mark; glyph/symbol/mark aliases accepted
//! color = "#F74C00"         # optional #RRGGBB, ANSI color name, or ANSI-256 index
//!
//! [observe]
//! mode = "subtree"          # subtree | exact | network
//! roots = ["../sibling"]    # extra roots when mode = "network"
//!
//! [scope]
//! build = true              # this node is the build-gate boundary; swallows
//!                           # every project beneath it into one gate unit
//! ```
//!
//! The protocol deliberately separates identity from liveness. Babel resolves
//! workgroup identity before paint events; panels consume the resolved
//! `workgroup_*` fields and keep animation/ring/outline for session state.
//!
//! It also separates identity from *concern boundaries*. A workgroup is a
//! group/identity mark, but "what root owns concern X here" is a per-[`Facet`]
//! question answered by [`boundary`] — builds bound at the nearest project by
//! default, group/presence at the nearest workgroup, and `[scope]` declarations
//! override per facet (innermost-wins). New concerns become new facets, not new
//! special cases.

use std::path::{Component, Path, PathBuf};

use serde::Serialize;

pub mod institution;
pub mod plugin;
pub mod screen;

pub const ORGMAP_FILE: &str = "orgmap.toml";
pub const WORKGROUP_FILE: &str = "workgroup.toml";
pub const HSP_WORKGROUP_FILE: &str = ".hsp/workgroup.toml";
pub const WORKGROUP_MARKERS: &[&str] = &[ORGMAP_FILE, WORKGROUP_FILE, HSP_WORKGROUP_FILE];

/// Structural signals that a directory is a buildable repo root. These define
/// the *implicit* `Build` facet boundary (the default gate unit) for any
/// directory that carries no explicit `[scope]` declaration — see [`boundary`].
/// A project is "an implicit workgroup with build-scoping but not
/// group-scoping": it bounds builds without forming a presence/identity group.
pub const PROJECT_MARKERS: &[&str] = &[".git", "Cargo.toml", "pyproject.toml", "package.json"];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum WorkgroupLevel {
    Umbrella,
    Domain,
    Project,
    Custom(String),
}

impl WorkgroupLevel {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "umbrella" => Self::Umbrella,
            "domain" => Self::Domain,
            "project" => Self::Project,
            other => Self::Custom(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Self::Umbrella => "umbrella",
            Self::Domain => "domain",
            Self::Project => "project",
            Self::Custom(value) => value.as_str(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ObservationMode {
    Exact,
    Subtree,
    Network,
}

impl ObservationMode {
    pub fn parse(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "exact" | "self" => Self::Exact,
            "network" | "roots" | "explicit" => Self::Network,
            _ => Self::Subtree,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Subtree => "subtree",
            Self::Network => "network",
        }
    }
}

/// A *concern* whose grouping boundary the org tree can answer for any path.
///
/// The org tree is one structure; different concerns bound at different
/// granularities. Rather than each consumer re-deriving "what root owns my
/// concern here" (the historical bodge — build gates reached for `workspace_root`,
/// presence hand-rolled [`ObservationMode`]), every concern names its facet and
/// asks [`boundary`]. New concerns are a new variant + a default floor, never a
/// new special case threaded through the bus.
///
/// `Presence` is reserved: it is `ObservationMode` + `observation_roots` waiting
/// to be folded into the same resolver. Until then it falls back to `Group`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Facet {
    /// The team room: presence, ticket visibility, identity (color/icon/name).
    /// Boundary = the nearest workgroup marker. Workgroups are *always* group
    /// boundaries — that is what a workgroup structurally is.
    Group,
    /// The build-gate unit: who must wait on whom before a build runs.
    /// Default boundary = the nearest structural project ([`PROJECT_MARKERS`]).
    /// An explicit `[scope] build` declaration overrides, innermost-wins, so a
    /// workgroup can *swallow* its projects into one unit and a project can
    /// *re-assert* itself out of a swallowing ancestor.
    Build,
    /// Reserved — see type docs. Resolves as `Group` until observation folds in.
    Presence,
}

/// Per-facet boundary declarations parsed from a node's `[scope]` table. Absent
/// keys mean "no opinion" — the facet falls to its default floor. This is the
/// extensible facet-map: a new facet is a new optional field here, not a new
/// `*_scope` key smeared across the schema.
///
/// ```toml
/// [scope]
/// build = true   # this node is the build-gate boundary; swallows everything beneath
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ScopeDecls {
    /// `Some(true)` — this node IS the build boundary (innermost such wins).
    /// `Some(false)` — explicitly NOT a boundary; defer the build unit upward.
    /// `None` — no opinion; structural project-marker status (if any) stands.
    pub build: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkgroupDefinition {
    pub root: PathBuf,
    pub marker: PathBuf,
    pub name: String,
    pub level: WorkgroupLevel,
    pub icon: Option<String>,
    pub color: Option<String>,
    pub ansi256: Option<u8>,
    pub observation_mode: ObservationMode,
    pub observation_roots: Vec<PathBuf>,
    /// Explicit per-facet boundary declarations from the `[scope]` table.
    pub scope: ScopeDecls,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WorkgroupIdentity {
    pub root: PathBuf,
    pub name: String,
    pub ansi256: u8,
    pub icon: Option<String>,
}

pub fn identity_for_path(path: &Path) -> Option<WorkgroupIdentity> {
    let definition = definition_for_path(path)?;
    Some(WorkgroupIdentity {
        root: definition.root,
        name: definition.name.clone(),
        ansi256: definition
            .ansi256
            .unwrap_or_else(|| auto_workgroup_ansi256(&definition.name)),
        icon: Some(definition.icon.unwrap_or_else(default_workgroup_icon)),
    })
}

pub fn definition_for_path(path: &Path) -> Option<WorkgroupDefinition> {
    let path = normalize_scope_path(path);
    for ancestor in path.ancestors() {
        if let Some(marker) = workgroup_marker(ancestor) {
            return read_definition(ancestor, marker);
        }
    }
    None
}

pub fn discover_workgroup_stack(path: &Path) -> Vec<WorkgroupDefinition> {
    let path = normalize_scope_path(path);
    let mut stack = Vec::new();
    for ancestor in path.ancestors() {
        if let Some(marker) = workgroup_marker(ancestor) {
            if let Some(definition) = read_definition(ancestor, marker) {
                stack.push(definition);
            }
        }
    }
    stack.reverse();
    stack
}

pub fn toml_path_for_path(path: &Path) -> Option<PathBuf> {
    let path = normalize_scope_path(path);
    for ancestor in path.ancestors() {
        if let Some(marker) = workgroup_marker(ancestor) {
            return Some(canonical_or_self(marker));
        }
    }
    None
}

/// Resolve the boundary root that owns `facet` at `path` — the one entry point
/// every bus concern uses instead of reaching for an accidental `workspace_root`
/// or hand-rolling its own tree walk. See [`Facet`] for per-facet semantics.
pub fn boundary(path: &Path, facet: Facet) -> PathBuf {
    let path = normalize_scope_path(path);
    match facet {
        // Presence is the reserved future fold of ObservationMode; until then it
        // shares the group boundary (the nearest workgroup).
        Facet::Group | Facet::Presence => definition_for_path(&path)
            .map(|definition| definition.root)
            .unwrap_or(path),
        Facet::Build => build_boundary(&path),
    }
}

/// `Build`-facet resolution, walking innermost → outermost:
/// - innermost explicit `[scope] build = true` wins — this is both *swallow*
///   (a workgroup claiming its subtree) and *re-assert* (a project claiming
///   itself back out of a swallowing ancestor); whichever is encountered first
///   from the build location is the unit,
/// - `build = false` opts a node out, deferring the unit upward,
/// - with no explicit declaration in the stack, the nearest structural project
///   ([`PROJECT_MARKERS`]) is the floor — the implicit project boundary,
/// - and failing even that, the path stands as its own boundary.
fn build_boundary(path: &Path) -> PathBuf {
    let mut project_floor: Option<PathBuf> = None;
    for ancestor in path.ancestors() {
        let declared = workgroup_marker(ancestor)
            .and_then(|marker| read_definition(ancestor, marker))
            .and_then(|definition| definition.scope.build);
        match declared {
            Some(true) => return canonical_or_self(ancestor.to_path_buf()),
            Some(false) => continue,
            None => {
                if project_floor.is_none() && is_project_root(ancestor) {
                    project_floor = Some(canonical_or_self(ancestor.to_path_buf()));
                }
            }
        }
    }
    project_floor.unwrap_or_else(|| path.to_path_buf())
}

/// True when a directory carries a structural build-system marker
/// ([`PROJECT_MARKERS`]) — the implicit `Build` boundary signal that makes a
/// plain repo its own build-gate unit without declaring anything.
pub fn is_project_root(path: &Path) -> bool {
    PROJECT_MARKERS
        .iter()
        .any(|marker| path.join(marker).exists())
}

pub fn default_workgroup_icon() -> String {
    // Explicit marks are identity. The fallback is generic so consumers do not
    // invent a visual taxonomy in paint clients.
    "\u{f02d8}".to_string()
}

pub fn auto_workgroup_ansi256(name: &str) -> u8 {
    const PALETTE: &[u8] = &[
        33, 39, 45, 69, 75, 81, 111, 117, 141, 147, 177, 183, 209, 215,
    ];
    let hash = name.bytes().fold(0usize, |acc, byte| {
        acc.wrapping_mul(33).wrapping_add(byte as usize)
    });
    PALETTE[hash % PALETTE.len()]
}

pub fn color_text_to_ansi256(text: &str) -> Option<u8> {
    let text = text.trim();
    if let Ok(ansi) = text.parse::<u8>() {
        return Some(ansi);
    }
    if text.starts_with('#') {
        return Some(theme_balanced_ansi256_from_hex(text));
    }
    match text.to_ascii_lowercase().as_str() {
        "black" => Some(0),
        "red" => Some(1),
        "green" => Some(2),
        "yellow" => Some(3),
        "blue" => Some(4),
        "magenta" => Some(5),
        "cyan" => Some(6),
        "white" => Some(7),
        "bright_black" | "gray" | "grey" => Some(8),
        "bright_red" => Some(9),
        "bright_green" => Some(10),
        "bright_yellow" => Some(11),
        "bright_blue" => Some(12),
        "bright_magenta" => Some(13),
        "bright_cyan" => Some(14),
        "bright_white" => Some(15),
        _ => None,
    }
}

fn workgroup_marker(parent: &Path) -> Option<PathBuf> {
    for marker in WORKGROUP_MARKERS {
        let path = parent.join(marker);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn read_definition(root: &Path, marker: PathBuf) -> Option<WorkgroupDefinition> {
    let text = std::fs::read_to_string(&marker).ok()?;
    let value = text.parse::<toml::Value>().ok()?;
    let table = value
        .get("orgmap")
        .or_else(|| value.get("workgroup"))
        .unwrap_or(&value);
    let observe = value.get("observe");
    let name = first_string(table, &["name"])
        .or_else(|| {
            root.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .filter(|name| !name.is_empty())?;
    let level = first_string(table, &["level"])
        .map(|level| WorkgroupLevel::parse(&level))
        .unwrap_or_else(|| default_level(root));
    let color = first_string(table, &["color", "fg", "foreground"]);
    let ansi256 = first_ansi256(table).or_else(|| color.as_deref().and_then(color_text_to_ansi256));

    Some(WorkgroupDefinition {
        root: canonical_or_self(root.to_path_buf()),
        marker: canonical_or_self(marker),
        name,
        level,
        icon: first_string(table, &["icon", "glyph", "symbol", "mark"]),
        color,
        ansi256,
        observation_mode: observation_mode(table, observe),
        observation_roots: observation_roots(root, table, observe),
        scope: scope_decls(value.get("scope"), table),
    })
}

/// Parse the `[scope]` facet-map. Primary source is the dedicated `[scope]`
/// table; `build_scope` in the `[workgroup]` table is accepted as a terse
/// flat alias so a one-liner override doesn't force a second table header.
fn scope_decls(scope: Option<&toml::Value>, table: &toml::Value) -> ScopeDecls {
    let build = scope
        .and_then(|scope| scope.get("build"))
        .or_else(|| table.get("build_scope"))
        .and_then(toml::Value::as_bool);
    ScopeDecls { build }
}

fn first_string(value: &toml::Value, keys: &[&str]) -> Option<String> {
    keys.iter()
        .filter_map(|key| value.get(*key))
        .filter_map(toml::Value::as_str)
        .map(str::trim)
        .find(|text| !text.is_empty())
        .map(str::to_string)
}

fn first_ansi256(value: &toml::Value) -> Option<u8> {
    for key in ["ansi256", "ansi", "ansi_color"] {
        if let Some(ansi) = value.get(key).and_then(toml_value_to_ansi256) {
            return Some(ansi);
        }
    }
    None
}

fn toml_value_to_ansi256(value: &toml::Value) -> Option<u8> {
    match value {
        toml::Value::Integer(i) => u8::try_from(*i).ok(),
        toml::Value::String(text) => color_text_to_ansi256(text),
        _ => None,
    }
}

fn observation_mode(table: &toml::Value, observe: Option<&toml::Value>) -> ObservationMode {
    first_string_from_tables(observe, table, &["mode", "observe", "observation"])
        .map(|mode| ObservationMode::parse(&mode))
        .unwrap_or(ObservationMode::Subtree)
}

fn observation_roots(
    root: &Path,
    table: &toml::Value,
    observe: Option<&toml::Value>,
) -> Vec<PathBuf> {
    let raw = observe
        .and_then(|observe| observe.get("roots"))
        .or_else(|| table.get("observe_roots"))
        .or_else(|| table.get("observation_roots"));

    let Some(raw) = raw else {
        return Vec::new();
    };

    string_list(raw)
        .into_iter()
        .map(|item| {
            let path = PathBuf::from(item);
            let absolute = if path.is_absolute() {
                path
            } else {
                root.join(path)
            };
            canonical_or_self(absolute)
        })
        .collect()
}

fn first_string_from_tables(
    observe: Option<&toml::Value>,
    table: &toml::Value,
    keys: &[&str],
) -> Option<String> {
    if let Some(observe) = observe {
        if let Some(value) = first_string(observe, keys) {
            return Some(value);
        }
    }
    first_string(table, keys)
}

fn string_list(value: &toml::Value) -> Vec<String> {
    match value {
        toml::Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                Vec::new()
            } else {
                vec![text.to_string()]
            }
        }
        toml::Value::Array(items) => items
            .iter()
            .filter_map(toml::Value::as_str)
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    }
}

fn default_level(root: &Path) -> WorkgroupLevel {
    if root
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("repo-"))
    {
        WorkgroupLevel::Domain
    } else {
        WorkgroupLevel::Umbrella
    }
}

fn normalize_scope_path(path: &Path) -> PathBuf {
    let normalized = normalize_lexical_path(path);
    if normalized.is_file() {
        normalized
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or(normalized)
    } else {
        normalized
    }
}

fn normalize_lexical_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = std::fs::canonicalize(path) {
        return canonical;
    }

    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            Component::RootDir => normalized.push(component.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
        }
    }
    normalized
}

fn canonical_or_self(path: PathBuf) -> PathBuf {
    std::fs::canonicalize(&path).unwrap_or(path)
}

fn theme_balanced_ansi256_from_hex(hex: &str) -> u8 {
    let rgb = hex_to_rgb(hex).unwrap_or((102, 102, 102));
    let balanced = balance_rgb_to_ansi_theme_luminance(rgb);
    closest_ansi256_from_rgb(balanced)
}

fn hex_to_rgb(hex: &str) -> Option<(u8, u8, u8)> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&hex[0..2], 16).ok()?;
    let g = u8::from_str_radix(&hex[2..4], 16).ok()?;
    let b = u8::from_str_radix(&hex[4..6], 16).ok()?;
    Some((r, g, b))
}

fn balance_rgb_to_ansi_theme_luminance(rgb: (u8, u8, u8)) -> (u8, u8, u8) {
    let source = perceptual_luminance(rgb);
    let target = ansi_chroma_average_luminance();
    if (source - target).abs() <= 0.01 {
        return rgb;
    }

    let target_rgb = if source < target {
        (255, 255, 255)
    } else {
        (0, 0, 0)
    };
    let mut low = 0.0;
    let mut high = 1.0;
    let mut best = rgb;

    for _ in 0..12 {
        let mid = (low + high) / 2.0;
        let candidate = lerp_rgb_f32(rgb, target_rgb, mid);
        best = candidate;
        let candidate_luma = perceptual_luminance(candidate);
        if source < target {
            if candidate_luma < target {
                low = mid;
            } else {
                high = mid;
            }
        } else if candidate_luma > target {
            low = mid;
        } else {
            high = mid;
        }
    }

    best
}

fn ansi_chroma_average_luminance() -> f32 {
    const CHROMA_INDICES: [u8; 12] = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14];
    let total: f32 = CHROMA_INDICES
        .iter()
        .map(|index| perceptual_luminance(ansi256_rgb(*index)))
        .sum();
    total / CHROMA_INDICES.len() as f32
}

fn closest_ansi256_from_rgb((r, g, b): (u8, u8, u8)) -> u8 {
    let ri = (((r as u16) * 5 + 127) / 255).min(5) as u8;
    let gi = (((g as u16) * 5 + 127) / 255).min(5) as u8;
    let bi = (((b as u16) * 5 + 127) / 255).min(5) as u8;
    16 + 36 * ri + 6 * gi + bi
}

fn ansi256_rgb(index: u8) -> (u8, u8, u8) {
    const ANSI16: [(u8, u8, u8); 16] = [
        (0, 0, 0),
        (128, 0, 0),
        (0, 128, 0),
        (128, 128, 0),
        (0, 0, 128),
        (128, 0, 128),
        (0, 128, 128),
        (192, 192, 192),
        (128, 128, 128),
        (255, 0, 0),
        (0, 255, 0),
        (255, 255, 0),
        (0, 0, 255),
        (255, 0, 255),
        (0, 255, 255),
        (255, 255, 255),
    ];

    match index {
        0..=15 => ANSI16[index as usize],
        16..=231 => {
            let idx = index - 16;
            let channel = |v: u8| if v == 0 { 0 } else { 55 + v * 40 };
            (channel(idx / 36), channel((idx / 6) % 6), channel(idx % 6))
        }
        232..=255 => {
            let shade = 8 + (index - 232) * 10;
            (shade, shade, shade)
        }
    }
}

fn lerp_rgb_f32(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    let channel = |from: u8, to: u8| {
        (from as f32 + (to as f32 - from as f32) * amount)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

fn perceptual_luminance((r, g, b): (u8, u8, u8)) -> f32 {
    let r = srgb_channel_to_linear(r);
    let g = srgb_channel_to_linear(g);
    let b = srgb_channel_to_linear(b);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

fn srgb_channel_to_linear(channel: u8) -> f32 {
    let value = channel as f32 / 255.0;
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_root(name: &str) -> PathBuf {
        let root = std::env::current_dir().unwrap().join("tmp").join(format!(
            "orgmap-{}-{}",
            name,
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    /// Default floor: a build under a plain workgroup resolves to its own
    /// project — gates are project-scoped without anyone declaring anything.
    #[test]
    fn build_facet_defaults_to_nearest_project() {
        let root = tmp_root("build-default");
        let workgroup = root.join("repo-os");
        let project = workgroup.join("babel");
        let src = project.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            workgroup.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\nlevel = \"domain\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(project.join(".git")).unwrap();

        let resolved = boundary(&src, Facet::Build);
        let expected = canonical_or_self(project.clone());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(resolved, expected);
    }

    /// `[scope] build = true` on the workgroup swallows its projects: a build in
    /// any child resolves to the workgroup, so the whole subtree gates as one.
    #[test]
    fn workgroup_scope_build_swallows_projects() {
        let root = tmp_root("build-swallow");
        let workgroup = root.join("repo-os");
        let babel = workgroup.join("babel");
        let src = babel.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            workgroup.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\nlevel = \"domain\"\n\n[scope]\nbuild = true\n",
        )
        .unwrap();
        std::fs::create_dir_all(babel.join(".git")).unwrap();

        let resolved = boundary(&src, Facet::Build);
        let expected = canonical_or_self(workgroup.clone());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(resolved, expected, "swallowing workgroup is the build boundary");
    }

    /// Innermost-explicit-wins: a project re-declaring `build = true` claims
    /// itself back out of a swallowing ancestor.
    #[test]
    fn project_reasserts_out_of_swallow() {
        let root = tmp_root("build-reassert");
        let workgroup = root.join("repo-os");
        let babel = workgroup.join("babel");
        let src = babel.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            workgroup.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\n\n[scope]\nbuild = true\n",
        )
        .unwrap();
        std::fs::create_dir_all(babel.join(".hsp")).unwrap();
        std::fs::write(
            babel.join(HSP_WORKGROUP_FILE),
            "[workgroup]\nname = \"babel\"\nlevel = \"project\"\n\n[scope]\nbuild = true\n",
        )
        .unwrap();

        let resolved = boundary(&src, Facet::Build);
        let expected = canonical_or_self(babel.clone());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(resolved, expected, "innermost explicit declaration wins");
    }

    /// `build = false` opts a project out of being its own unit, deferring the
    /// build boundary up to the swallowing parent.
    #[test]
    fn scope_build_false_defers_upward() {
        let root = tmp_root("build-optout");
        let workgroup = root.join("repo-os");
        let babel = workgroup.join("babel");
        let src = babel.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(
            workgroup.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\n\n[scope]\nbuild = true\n",
        )
        .unwrap();
        std::fs::create_dir_all(babel.join(".git")).unwrap();
        std::fs::write(
            babel.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"babel\"\n\n[scope]\nbuild = false\n",
        )
        .unwrap();

        let resolved = boundary(&src, Facet::Build);
        let expected = canonical_or_self(workgroup.clone());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(resolved, expected, "build=false defers the unit to the parent");
    }

    /// The terse flat alias `build_scope = true` in `[workgroup]` parses the
    /// same as the `[scope]` table — a one-liner override needs no second header.
    #[test]
    fn flat_build_scope_alias_parses() {
        let root = tmp_root("build-flat-alias");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::write(
            root.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"mono\"\nbuild_scope = true\n",
        )
        .unwrap();
        let definition = definition_for_path(&root.join("sub")).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(definition.scope.build, Some(true));
    }

    #[test]
    fn reads_nearest_identity_icon_and_color() {
        let root = tmp_root("identity");
        let project = root.join("repo").join("src");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(
            root.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\nicon = \"*\"\nansi256 = 39\n",
        )
        .unwrap();

        let identity = identity_for_path(&project).unwrap();
        std::fs::remove_dir_all(root.clone()).unwrap();

        assert_eq!(identity.root, root);
        assert_eq!(identity.name, "repo-os");
        assert_eq!(identity.icon.as_deref(), Some("*"));
        assert_eq!(identity.ansi256, 39);
    }

    #[test]
    fn discovers_nested_stack_and_hsp_marker() {
        let root = tmp_root("stack");
        let domain = root.join("repo-os");
        let project = domain.join("babel");
        std::fs::create_dir_all(project.join("src")).unwrap();
        std::fs::write(
            root.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"holoq\"\nlevel = \"umbrella\"\n",
        )
        .unwrap();
        std::fs::create_dir_all(domain.join(".hsp")).unwrap();
        std::fs::write(
            domain.join(HSP_WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\nlevel = \"domain\"\n",
        )
        .unwrap();

        let stack = discover_workgroup_stack(&project);
        std::fs::remove_dir_all(root).unwrap();

        let names = stack
            .iter()
            .map(|item| item.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.ends_with(&["holoq", "repo-os"]));
        assert_eq!(stack[stack.len() - 2].level, WorkgroupLevel::Umbrella);
        assert_eq!(stack[stack.len() - 1].level, WorkgroupLevel::Domain);
    }

    #[test]
    fn resolves_observation_roots_relative_to_marker_root() {
        let root = tmp_root("observe");
        let domain = root.join("repo-os");
        let sibling = root.join("repo-agent");
        std::fs::create_dir_all(&domain).unwrap();
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::write(
            domain.join(WORKGROUP_FILE),
            "[workgroup]\nname = \"repo-os\"\n[observe]\nmode = \"network\"\nroots = [\"../repo-agent\"]\n",
        )
        .unwrap();

        let definition = definition_for_path(&domain).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(definition.observation_mode, ObservationMode::Network);
        assert_eq!(definition.observation_roots, vec![sibling]);
    }

    #[test]
    fn orgmap_table_takes_precedence_over_legacy_workgroup_table() {
        let root = tmp_root("orgmap-precedence");
        std::fs::write(
            root.join(ORGMAP_FILE),
            "[orgmap]\nname = \"orgmap-name\"\n[workgroup]\nname = \"legacy-name\"\n",
        )
        .unwrap();

        let definition = definition_for_path(&root).unwrap();
        std::fs::remove_dir_all(root).unwrap();

        assert_eq!(definition.name, "orgmap-name");
    }

    #[test]
    fn default_icon_stays_in_nerd_font_private_use_range() {
        let icon = default_workgroup_icon();
        let codepoint = icon.chars().next().unwrap() as u32;
        assert!((0xf0000..=0x10ffff).contains(&codepoint));
    }
}
