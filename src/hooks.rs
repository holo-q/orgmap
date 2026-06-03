//! `org hooks` — install + inspect the org's universal git hooks.
//!
//! A single dispatcher script (`assets/org-hook-dispatch`, embedded here via
//! `include_str!`) is written to a hooks dir and symlinked under every standard
//! hook name; `core.hooksPath` is pointed at that dir globally, so EVERY repo —
//! current and future clones — gets the pre-push secret gate automatically, with
//! no per-repo install. The dispatcher chains to each repo's own hooks so
//! existing husky/pre-commit setups keep working (see the script header).
//!
//! `org screen .` (path mode) is what the gate calls; this module just installs
//! the plumbing that invokes it on push.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The dispatcher script, tracked as a real shell file and baked into the binary
/// so `org hooks install` is self-contained (no external file to ship).
const DISPATCH: &str = include_str!("../assets/org-hook-dispatch");

/// Every standard client-side hook name. The dispatcher is a no-op for all but
/// `pre-push` (the gate) UNLESS the repo has its own hook of that name, which it
/// then chains to — so symlinking the full set maximises preservation of
/// existing per-repo hooks that the global core.hooksPath would otherwise hide.
const HOOK_NAMES: &[&str] = &[
    "pre-push",
    "pre-commit",
    "commit-msg",
    "prepare-commit-msg",
    "pre-rebase",
    "pre-merge-commit",
    "post-commit",
    "post-checkout",
    "post-merge",
    "post-rewrite",
    "applypatch-msg",
    "pre-applypatch",
    "post-applypatch",
    "pre-auto-gc",
];

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default()
}

/// Default hooks dir — XDG-based and portable (`$XDG_CONFIG_HOME/org/git-hooks`,
/// else `~/.config/org/git-hooks`). Override with `--dir`.
pub fn default_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join(".config"));
    base.join("org").join("git-hooks")
}

/// Write the dispatcher, (re)create the hook symlinks, and point global
/// `core.hooksPath` at `dir`. Idempotent — safe to re-run to refresh the script.
pub fn install(dir: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;

    let script = dir.join("org-hook-dispatch");
    fs::write(&script, DISPATCH)?;
    let mut perm = fs::metadata(&script)?.permissions();
    perm.set_mode(0o755);
    fs::set_permissions(&script, perm)?;

    for name in HOOK_NAMES {
        let link = dir.join(name);
        let _ = fs::remove_file(&link); // replace any stale link/file
        symlink("org-hook-dispatch", &link)?;
    }

    let ok = Command::new("git")
        .args(["config", "--global", "core.hooksPath"])
        .arg(dir)
        .status()?
        .success();
    if !ok {
        return Err(std::io::Error::other(
            "git config --global core.hooksPath failed",
        ));
    }

    println!("✓ org hooks installed");
    println!("  dispatcher : {}", script.display());
    println!("  hooks wired: {}", HOOK_NAMES.join(", "));
    println!("  core.hooksPath (global) → {}", dir.display());
    println!("  pre-push now runs `org screen . --secrets-only` on every org-repo push.");
    Ok(())
}

/// Report the current installation state.
pub fn status(dir: &Path) {
    let configured = Command::new("git")
        .args(["config", "--global", "--get", "core.hooksPath"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    println!("org hooks status");
    match &configured {
        Some(p) => println!("  core.hooksPath (global) = {p}"),
        None => println!("  core.hooksPath (global) = <unset>"),
    }
    let script = dir.join("org-hook-dispatch");
    println!(
        "  dispatcher present     = {} ({})",
        script.exists(),
        script.display()
    );
    let wired: Vec<&str> = HOOK_NAMES
        .iter()
        .copied()
        .filter(|n| dir.join(n).exists())
        .collect();
    println!("  hooks wired ({}/{})    = {}", wired.len(), HOOK_NAMES.len(), wired.join(", "));
    let active = configured.as_deref() == Some(&*dir.to_string_lossy());
    println!(
        "  active                 = {}",
        if active { "yes" } else { "no (core.hooksPath points elsewhere)" }
    );
}
