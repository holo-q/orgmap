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
/// - with no explicit declaration, the floor is the nearest **git repository
///   root** — in this org a *project* is a git repo, and a cargo/uv workspace's
///   member crates (each with their own `Cargo.toml`/`pyproject.toml` but no
///   `.git`) are internal sub-units that share one compile graph, so they must
///   gate as the repo, not fragment per-member,
/// - failing a `.git` ancestor, the nearest other structural marker
///   ([`PROJECT_MARKERS`]) is the fallback (uncommitted scaffolding),
/// - and failing even that, the path stands as its own boundary.
fn build_boundary(path: &Path) -> PathBuf {
    // `.git` is the truthful project signal; other markers are a weaker
    // fallback so a member `Cargo.toml` never out-votes its enclosing repo.
    let mut git_floor: Option<PathBuf> = None;
    let mut marker_floor: Option<PathBuf> = None;
    for ancestor in path.ancestors() {
        let declared = workgroup_marker(ancestor)
            .and_then(|marker| read_definition(ancestor, marker))
            .and_then(|definition| definition.scope.build);
        match declared {
            Some(true) => return canonical_or_self(ancestor.to_path_buf()),
            Some(false) => continue,
            None => {
                if git_floor.is_none() && ancestor.join(".git").exists() {
                    git_floor = Some(canonical_or_self(ancestor.to_path_buf()));
                }
                if marker_floor.is_none() && is_project_root(ancestor) {
                    marker_floor = Some(canonical_or_self(ancestor.to_path_buf()));
                }
            }
        }
    }
    git_floor
        .or(marker_floor)
        .unwrap_or_else(|| path.to_path_buf())
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

/// A project's position in the org's `[sections]` pecking order: which section
/// block (in *declaration order*, not alphabetical) and which index within it.
///
/// This is the rank babel's HUD sorts by — orgmap-ranked projects float above
/// the unranked rest. It was historically reimplemented as a hand-rolled
/// `[sections]` line-scanner in babel (`format.rs` / `ProjectOrder.cs`); this is
/// the canonical Rust truth that the C# binding now consumes over FFI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SectionRank {
    /// Section block index in *file declaration order* (first `name = [...]`
    /// array encountered is section 0). NOT the alphabetical BTreeMap order —
    /// the visual pecking order is the order sections are written.
    pub section: usize,
    /// Project index within its section block (declaration order).
    pub project: usize,
}

/// Read an `orgmap.toml`'s `[sections]` table into a project → [`SectionRank`]
/// map. Mirrors babel's former `parse_orgmap_ranks`:
///
/// - only the `[sections]` table matters; each `name = [ "stage:project", … ]`
///   array assigns the next sequential **section index in declaration order**
///   and per-entry **project indices**,
/// - the `stage:` prefix (e.g. `"3:babel"`) is stripped to the bare project
///   name (`babel`),
/// - first occurrence of a project name wins; later duplicates are ignored.
///
/// A missing/unreadable/sections-less file yields an empty map (best-effort) —
/// the HUD treats an unranked project as tier-1 (below all ranked ones).
///
/// Declaration order is load-bearing — and it is precisely what a structural
/// TOML parse would *destroy*. `toml::Value`'s table is a `BTreeMap` (sorted by
/// key), so `[sections]` arrays would come back alphabetized, scrambling the
/// pecking order; `institution::OrgConfig::sections` has the same BTreeMap flaw.
/// So this is a deliberate **line scanner** over the raw text (the canonical home
/// of babel's former `parse_orgmap_ranks`), tracking the open `[sections]` block
/// and the cursor inside an open array literal — preserving the order arrays and
/// entries are *written*, which is the order users intend.
pub fn section_ranks(orgmap_toml: &Path) -> std::collections::BTreeMap<String, SectionRank> {
    let mut ranks = std::collections::BTreeMap::new();
    let Ok(text) = std::fs::read_to_string(orgmap_toml) else {
        return ranks;
    };

    let mut in_sections = false;
    let mut section_index: usize = 0;
    // `cursor` = the (section, next-project) position while inside an open array
    // literal; `None` between arrays.
    let mut cursor: Option<(usize, usize)> = None;

    for raw_line in text.lines() {
        // Strip an inline `#` comment, then trim.
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }

        if line.starts_with('[') {
            in_sections = line == "[sections]";
            cursor = None;
            continue;
        }
        if !in_sections {
            continue;
        }

        // A `name = [` line opens a new section block at the next sequential index.
        if let Some((_, rest)) = line.split_once('=') {
            if rest.contains('[') {
                cursor = Some((section_index, 0));
                section_index += 1;
            }
        }

        if let Some((section, mut next_project)) = cursor {
            for entry in quoted_tokens(line) {
                let name = section_entry_project_name(&entry);
                if name.is_empty() {
                    continue;
                }
                // First occurrence wins (matches babel's historical `or_insert`).
                ranks.entry(name).or_insert(SectionRank {
                    section,
                    project: next_project,
                });
                next_project += 1;
            }
            cursor = Some((section, next_project));
        }

        // A `]` closes the current array literal.
        if line.contains(']') {
            cursor = None;
        }
    }

    ranks
}

/// Pull every double-quoted token out of a line. Mirrors babel's `quoted_tokens`
/// — used by [`section_ranks`] to lift `"stage:project"` entries out of an array
/// literal without a full TOML parse (which would lose declaration order).
fn quoted_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut rest = line;
    while let Some(start) = rest.find('"') {
        let after_start = &rest[start + 1..];
        let Some(end) = after_start.find('"') else {
            break;
        };
        tokens.push(after_start[..end].to_string());
        rest = &after_start[end + 1..];
    }
    tokens
}

/// Strip an orgmap `stage:` prefix (e.g. `"3:babel"` → `"babel"`), returning the
/// trimmed bare project name. Mirrors babel's `orgmap_project_name`.
fn section_entry_project_name(entry: &str) -> String {
    match entry.split_once(':') {
        Some((_stage, rest)) => rest.trim().to_string(),
        None => entry.trim().to_string(),
    }
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
        .unwrap_or_else(default_level);
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

/// Fallback workgroup level when `workgroup.toml` declares no explicit `level`
/// (an explicit declaration always wins — see the `first_string(table,
/// &["level"])` call site). Deliberately NAME-AGNOSTIC: orgmap assumes nothing
/// about an adopter's directory naming. (holoq's `repo-*` convention used to be
/// inferred here as Domain — a holoq-ism that mis-leveled any org naming its
/// domains differently, e.g. `service-auth`. holoq's `repo-*` dirs all declare
/// `level = "domain"` explicitly, so dropping the heuristic changes nothing for
/// holoq while unblocking everyone else.) Declare `level` per workgroup.toml;
/// an undeclared dir defaults to the neutral Umbrella.
fn default_level() -> WorkgroupLevel {
    WorkgroupLevel::Umbrella
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

    /// A cargo/uv workspace member (its own `Cargo.toml`, no `.git`) resolves to
    /// the enclosing git repo, not the member — members share one compile graph
    /// and must gate as the project, not fragment per-crate.
    #[test]
    fn build_facet_climbs_past_workspace_members_to_git_repo() {
        let root = tmp_root("build-workspace-member");
        let repo = root.join("hsp");
        let member = repo.join("crates").join("hsp-bus");
        let src = member.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join("Cargo.toml"), "[workspace]\n").unwrap();
        std::fs::write(member.join("Cargo.toml"), "[package]\nname = \"hsp-bus\"\n").unwrap();

        let resolved = boundary(&src, Facet::Build);
        let expected = canonical_or_self(repo.clone());
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(resolved, expected, "member crate gates as the git repo");
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

    /// `section_ranks` must rank by **declaration order** of the `[sections]`
    /// blocks, NOT alphabetically — the visual pecking order is the order the
    /// arrays are written. This is the whole reason it parses `toml::Value`
    /// directly instead of routing through `institution::OrgConfig` (whose
    /// `sections` BTreeMap would re-sort alphabetically). The probe writes
    /// `zeta` before `alpha`: a correct reading puts zeta's project at section 0.
    #[test]
    fn section_ranks_follow_declaration_order_not_alphabetical() {
        let root = tmp_root("section-ranks");
        let orgmap = root.join(ORGMAP_FILE);
        std::fs::write(
            &orgmap,
            "[sections]\nzeta = [\"3:zz\", \"2:zz2\"]\nalpha = [\"3:aa\"]\n",
        )
        .unwrap();

        let ranks = section_ranks(&orgmap);
        std::fs::remove_dir_all(&root).unwrap();

        // zeta is declared first ⇒ section 0; its two entries are projects 0, 1.
        assert_eq!(ranks["zz"], SectionRank { section: 0, project: 0 });
        assert_eq!(ranks["zz2"], SectionRank { section: 0, project: 1 });
        // alpha is declared second ⇒ section 1 even though 'alpha' < 'zeta'.
        assert_eq!(ranks["aa"], SectionRank { section: 1, project: 0 });
    }

    /// The `stage:` prefix is stripped to the bare project name, and the first
    /// occurrence of a duplicate name wins (matching babel's historical `or_insert`).
    #[test]
    fn section_ranks_strip_stage_and_keep_first_occurrence() {
        let root = tmp_root("section-ranks-dup");
        let orgmap = root.join(ORGMAP_FILE);
        std::fs::write(
            &orgmap,
            "[sections]\nfirst = [\"3:babel\"]\nsecond = [\"2:babel\"]\n",
        )
        .unwrap();

        let ranks = section_ranks(&orgmap);
        std::fs::remove_dir_all(&root).unwrap();

        // Bare name recovered; first occurrence (section 0) wins over the later dup.
        assert_eq!(ranks["babel"], SectionRank { section: 0, project: 0 });
        assert_eq!(ranks.len(), 1);
    }

    #[test]
    fn default_icon_stays_in_nerd_font_private_use_range() {
        let icon = default_workgroup_icon();
        let codepoint = icon.chars().next().unwrap() as u32;
        assert!((0xf0000..=0x10ffff).contains(&codepoint));
    }
}
