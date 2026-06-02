//! orgmap-ffi — the C-ABI face of the [`orgmap`] workgroup-identity protocol.
//!
//! This crate is a thin `extern "C"` shim over `orgmap`'s pub fns; it owns no
//! logic of its own. rustbind's `manifest-gen` parses *this* crate's `src/lib.rs`
//! (not orgmap's) to emit the typed `bindings.json` → C# `Native.cs`. So the
//! whole ABI contract lives here, in one file, as the manifest-gen requires:
//! every binding-bearing function is a top-level `#[no_mangle] pub extern "C" fn`.
//!
//! ## Marshalling convention
//!
//! - **Structs with String fields** (`WorkgroupIdentity`, the section-rank map)
//!   cross the boundary as **JSON in a `*mut c_char`**. The caller parses the
//!   JSON and MUST free the buffer with [`orgmap_string_free`]. This is the
//!   simplest faithful crossing for variable-width owned data — no manual struct
//!   layout, no per-field free dance.
//! - **Scalars** (`color_text_to_ansi256` → `u8`) pass directly; the `Option` is
//!   modelled as a `bool` return + a `*mut u8` out-param (`true` ⇒ out written).
//!
//! ## Why mirror babel's three reimplementations
//!
//! babel hand-rolled this surface in three places (project-metrics identity +
//! `#RRGGBB`→ANSI256 perceptual bisection, hud `[sections]` rank loader, world
//! workgroup-paint stub). Each was a faithful-but-lossy port (the hex path fell
//! back to a cube quantizer because the theme-balanced luminance bisection "had
//! no C# home"). Routing all three through this FFI makes the Rust engine the
//! single source of truth and recovers the exact perceptual conversion for free.

use std::ffi::{c_char, CStr, CString};
use std::path::Path;
use std::ptr;

use orgmap::{Facet, WorkgroupDefinition};
use serde::Serialize;

/// Convert a borrowed C string to a Rust `&str`, or `None` on null / non-UTF-8.
///
/// # Safety
/// `ptr` must be null or a valid NUL-terminated C string the caller keeps alive
/// for the duration of the call.
unsafe fn cstr_to_str<'a>(ptr: *const c_char) -> Option<&'a str> {
    if ptr.is_null() {
        return None;
    }
    CStr::from_ptr(ptr).to_str().ok()
}

/// Allocate an owned C string from a Rust string, transferring ownership to the
/// caller (free via [`orgmap_string_free`]). Returns null if the string contains
/// an interior NUL (cannot happen for our JSON, but we never panic over FFI).
fn into_c_char(text: String) -> *mut c_char {
    match CString::new(text) {
        Ok(c) => c.into_raw(),
        Err(_) => ptr::null_mut(),
    }
}

/// Serializable mirror of [`orgmap::WorkgroupIdentity`] with the `PathBuf` root
/// rendered as a display string — the JSON shape the C# ergonomic layer parses.
#[derive(Serialize)]
struct IdentityJson {
    root: String,
    name: String,
    ansi256: u8,
    icon: Option<String>,
}

/// Resolve a path to its owning workgroup identity, returned as JSON
/// (`{"root","name","ansi256","icon"}`) in a freshly-allocated C string, or
/// **null** when no workgroup marker encloses the path.
///
/// Wraps [`orgmap::identity_for_path`]. Free the result with
/// [`orgmap_string_free`].
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 C string.
#[no_mangle]
pub extern "C" fn orgmap_identity_for_path(path: *const c_char) -> *mut c_char {
    let Some(path) = (unsafe { cstr_to_str(path) }) else {
        return ptr::null_mut();
    };
    let Some(identity) = orgmap::identity_for_path(Path::new(path)) else {
        return ptr::null_mut();
    };
    let json = IdentityJson {
        root: identity.root.display().to_string(),
        name: identity.name,
        ansi256: identity.ansi256,
        icon: identity.icon,
    };
    match serde_json::to_string(&json) {
        Ok(text) => into_c_char(text),
        Err(_) => ptr::null_mut(),
    }
}

/// Parse a color token (bare ANSI index, `#RRGGBB` hex, or named ANSI color) to
/// an ansi256 index. Returns `true` and writes the index to `out_ansi` when the
/// token resolves; returns `false` (leaving `out_ansi` untouched) otherwise.
///
/// Wraps [`orgmap::color_text_to_ansi256`] — including the theme-balanced
/// perceptual-luminance hex→ansi256 bisection that babel's C# port could not
/// replicate.
///
/// # Safety
/// `text` must be a valid NUL-terminated UTF-8 C string; `out_ansi` must be a
/// valid, writable `*mut u8`.
#[no_mangle]
pub extern "C" fn orgmap_color_text_to_ansi256(text: *const c_char, out_ansi: *mut u8) -> bool {
    let Some(text) = (unsafe { cstr_to_str(text) }) else {
        return false;
    };
    match orgmap::color_text_to_ansi256(text) {
        Some(ansi) => {
            if !out_ansi.is_null() {
                unsafe { *out_ansi = ansi };
            }
            true
        }
        None => false,
    }
}

/// One project's `[sections]` pecking-order position, as JSON-serialized for the
/// section-rank map. Mirrors [`orgmap::SectionRank`] plus the project name key.
#[derive(Serialize)]
struct SectionRankJson {
    name: String,
    section: usize,
    project: usize,
}

/// Read an `orgmap.toml`'s `[sections]` table into a JSON **array** of
/// `{"name","section","project"}` rows (section/project in file declaration
/// order), in a freshly-allocated C string. Never null for a valid call: a
/// missing/unreadable/sections-less file yields `"[]"`.
///
/// Wraps [`orgmap::section_ranks`]. Free the result with [`orgmap_string_free`].
/// An array (not an object) is used so the C# side can build its dictionary
/// without imposing JSON-object key ordering semantics.
///
/// # Safety
/// `orgmap_toml_path` must be a valid NUL-terminated UTF-8 C string.
#[no_mangle]
pub extern "C" fn orgmap_section_ranks(orgmap_toml_path: *const c_char) -> *mut c_char {
    let path = unsafe { cstr_to_str(orgmap_toml_path) }.unwrap_or("");
    let ranks = orgmap::section_ranks(Path::new(path));
    let rows: Vec<SectionRankJson> = ranks
        .into_iter()
        .map(|(name, rank)| SectionRankJson {
            name,
            section: rank.section,
            project: rank.project,
        })
        .collect();
    match serde_json::to_string(&rows) {
        Ok(text) => into_c_char(text),
        Err(_) => into_c_char("[]".to_string()),
    }
}

/// Serializable mirror of [`orgmap::WorkgroupDefinition`] — the JSON shape the
/// C# ergonomic layer parses. Like [`IdentityJson`], `PathBuf` roots render as
/// display strings, and the `level`/`observation_mode` enums collapse to their
/// `as_str()` token (`"umbrella"`, `"exact"`, …) rather than crossing as serde's
/// externally-tagged enum reprs (which would leak `{"Custom": "…"}` /
/// `"Umbrella"` quirks the C# side should never have to know). The `[scope]`
/// declarations flatten to a single `build_scope` tri-state (`null` = no opinion,
/// `true`/`false` = explicit boundary opt-in/out) mirroring [`orgmap::ScopeDecls`].
#[derive(Serialize)]
struct WorkgroupDefinitionJson {
    root: String,
    marker: String,
    name: String,
    level: String,
    icon: Option<String>,
    color: Option<String>,
    ansi256: Option<u8>,
    observation_mode: String,
    observation_roots: Vec<String>,
    build_scope: Option<bool>,
}

impl From<WorkgroupDefinition> for WorkgroupDefinitionJson {
    fn from(definition: WorkgroupDefinition) -> Self {
        Self {
            root: definition.root.display().to_string(),
            marker: definition.marker.display().to_string(),
            name: definition.name,
            level: definition.level.as_str().to_string(),
            icon: definition.icon,
            color: definition.color,
            ansi256: definition.ansi256,
            observation_mode: definition.observation_mode.as_str().to_string(),
            observation_roots: definition
                .observation_roots
                .into_iter()
                .map(|root| root.display().to_string())
                .collect(),
            build_scope: definition.scope.build,
        }
    }
}

/// Parse a facet wire token to the [`orgmap::Facet`] enum. The build-gate /
/// "who-waits-on-whom" resolver is the primary consumer, so an unknown/empty
/// token defaults to [`Facet::Build`] (the spec's stable default), never panics.
fn parse_facet(token: Option<&str>) -> Facet {
    match token.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("group") => Facet::Group,
        Some("presence") => Facet::Presence,
        // "build" and any unrecognized/missing token resolve to the Build facet.
        _ => Facet::Build,
    }
}

/// Resolve the boundary root that owns `facet` at `path`, returned as JSON
/// (`{"root":"…"}`) in a freshly-allocated C string. `facet` is a wire token
/// (`"group"`|`"build"`|`"presence"`); unknown/null defaults to **build** (the
/// build-gate boundary — the "who-waits-on-whom" unit). Never null for a valid
/// `path`: [`orgmap::boundary`] always yields a path (the path itself is the
/// floor). A null/non-UTF-8 `path` yields null.
///
/// Wraps [`orgmap::boundary`]. Free the result with [`orgmap_string_free`].
///
/// # Safety
/// `path` and `facet` must be null or valid NUL-terminated UTF-8 C strings.
#[no_mangle]
pub extern "C" fn orgmap_boundary(path: *const c_char, facet: *const c_char) -> *mut c_char {
    let Some(path) = (unsafe { cstr_to_str(path) }) else {
        return ptr::null_mut();
    };
    let facet = parse_facet(unsafe { cstr_to_str(facet) });
    let root = orgmap::boundary(Path::new(path), facet);
    // A bare `{"root": "…"}` object mirrors IdentityJson's shape conventions; an
    // object (not a raw string) leaves room for the resolver to grow without an
    // ABI break.
    match serde_json::to_string(&serde_json::json!({ "root": root.display().to_string() })) {
        Ok(text) => into_c_char(text),
        Err(_) => ptr::null_mut(),
    }
}

/// Resolve a path to its owning workgroup definition, returned as JSON (the
/// [`WorkgroupDefinitionJson`] shape) in a freshly-allocated C string, or
/// **null** when no workgroup marker encloses the path.
///
/// Wraps [`orgmap::definition_for_path`]. Free the result with
/// [`orgmap_string_free`].
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 C string.
#[no_mangle]
pub extern "C" fn orgmap_definition_for_path(path: *const c_char) -> *mut c_char {
    let Some(path) = (unsafe { cstr_to_str(path) }) else {
        return ptr::null_mut();
    };
    let Some(definition) = orgmap::definition_for_path(Path::new(path)) else {
        return ptr::null_mut();
    };
    match serde_json::to_string(&WorkgroupDefinitionJson::from(definition)) {
        Ok(text) => into_c_char(text),
        Err(_) => ptr::null_mut(),
    }
}

/// Resolve the full ancestor stack of workgroup definitions enclosing `path`,
/// outermost → innermost, returned as a JSON **array** of
/// [`WorkgroupDefinitionJson`] in a freshly-allocated C string. Never null for a
/// valid call: a path with no enclosing markers yields `"[]"`.
///
/// Wraps [`orgmap::discover_workgroup_stack`]. Free the result with
/// [`orgmap_string_free`].
///
/// # Safety
/// `path` must be a valid NUL-terminated UTF-8 C string.
#[no_mangle]
pub extern "C" fn orgmap_discover_workgroup_stack(path: *const c_char) -> *mut c_char {
    let path = unsafe { cstr_to_str(path) }.unwrap_or("");
    let stack: Vec<WorkgroupDefinitionJson> = orgmap::discover_workgroup_stack(Path::new(path))
        .into_iter()
        .map(WorkgroupDefinitionJson::from)
        .collect();
    match serde_json::to_string(&stack) {
        Ok(text) => into_c_char(text),
        Err(_) => into_c_char("[]".to_string()),
    }
}

/// Free a C string previously returned by an `orgmap_*` function. Null-safe.
///
/// # Safety
/// `s` must be null or a pointer returned by this library's `into_c_char` (i.e.
/// `CString::into_raw`), freed exactly once.
#[no_mangle]
pub extern "C" fn orgmap_string_free(s: *mut c_char) {
    if !s.is_null() {
        unsafe {
            drop(CString::from_raw(s));
        }
    }
}
