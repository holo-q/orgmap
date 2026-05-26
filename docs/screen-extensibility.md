# `org screen` extensibility — patterns & screeners

## Why

`org screen` ships **universal ship-safety invariants** that every software
org wants regardless of language or taste: leaked secrets, identity/path
leaks. That is the *only* thing orgmap core should be opinionated about,
because orgmap is meant to be adopted by **anyone** to bootstrap a software
organization — not to encode holoq's aesthetics or assume Rust.

But orgs legitimately want to enforce their *own* invariants (a Rust shop's
"no logic in mod.rs", a Python shop's "no `print()` in libs", a company's
"no `corp.internal` hostnames in public repos"). Those must NOT enter orgmap
core. The answer is an **extension seam**: orgmap ships the universal lane +
two ways for an org to plug in its own rules, declared entirely in that org's
`orgmap.toml` (+ that org's own scripts). holoq's mod.rs-purity law then
becomes a holoq-provided *script*, never a line of orgmap source.

The embryo of both mechanisms already exists in `screen.rs`; this is a
generalization, not a greenfield feature.

## Current shape (what we generalize)

| Today (hardcoded) | Becomes (general) |
|---|---|
| `build_static_patterns()` — baked secret/sloppy regexes | stays: orgmap's **universal** lane (the only baked opinions) |
| `build_dynamic_patterns(cfg)` — identity-derived patterns | also ingests org-declared **`[[screen.pattern]]`** entries |
| `run_gitleaks()` / `which_gitleaks()` / `GitleaksRecord` | one built-in **screener** registration behind a generic registry |
| `FindingSource { Pattern, Gitleaks }` | `FindingSource { Pattern, Screener(name) }` |
| `ScreenOptions.no_gitleaks` | `ScreenOptions.skip_screeners: Vec<String>` (`--no-gitleaks` kept as alias) |
| `ProjectFindings.gitleaks_error: Option<String>` | `screener_errors: BTreeMap<name, String>` |

The principle: **gitleaks is not special — it is the first instance of the
external-screener registry.** If gitleaks still works after being rebuilt as
a generic registration, the seam is real (dogfood proof).

## Lane A — declarative patterns (`[[screen.pattern]]`)

Pure data; language-agnostic; safe (regex only, no code execution). Appended
to the dynamic-pattern set.

```toml
[[screen.pattern]]
id            = "internal_host"
severity      = "warn"          # critical | warn | info
regex         = 'corp\.internal'
docs_downgrade = true           # downgrade → info inside doc files (md/txt/rst)
secrets_only  = false           # survive the --secrets-only gate?
```

Implementation: extend `build_dynamic_patterns(cfg)` to push a `Pattern` per
entry. The `docs_downgrade` / `secrets_only` behaviors are currently *implicit
per family* — make them explicit per-pattern attributes, with the universal
baked patterns keeping today's defaults.

## Lane B — external screeners (`[[screen.screener]]`)

Generalizes the gitleaks shell-out. An org registers an external command;
orgmap discovers it, runs it per project, ingests its findings, merges them
into the tiered report. This is how a Rust shop plugs in a mod.rs linter, a
Python shop a `print()` check, etc. — **orgmap never learns the language.**

```toml
# built-in default registration — auto-injected, so out-of-box is unchanged.
# (Shown for reference; an org only writes this to redefine gitleaks.)
[[screen.screener]]
name     = "gitleaks"
command  = "gitleaks"           # the gitleaks adapter bakes its own version-stable invocation; `args` is ignored
adapter  = "gitleaks"           # built-in translator for this tool's native JSON report
optional = true                 # binary missing → skip, don't fail
severity = "critical"           # default tier for findings from this screener

# an ORG's own rule — lives in the org's orgmap.toml, points at the org's script
[[screen.screener]]
name     = "modrs-purity"
command  = "{org_root}/tooling/screen-modrs"
args     = ["{project}"]        # placeholders substituted at run time
adapter  = "orgmap"             # script speaks the orgmap finding protocol
```

Placeholders substituted at run time (orgmap adapter): `{project}` (abs path
to the project being scanned), `{org_root}` (orgmap roots base). The screener
also runs with its cwd set to the project. Missing-binary handling is governed
by `optional`. A nonzero exit (orgmap adapter) is recorded as a per-screener
error (today's `gitleaks_error`, generalized to `screener_errors[name]`).

Screeners run only on git-tracked projects — the scan early-returns on a
non-git tree, so a `requires` gate would be dead config; it's structural.

### Directory auto-discovery (`screener_dirs`)

Listing each script as a `[[screen.screener]]` block gets tedious once an org
has several. `screener_dirs` globs a directory instead — every executable file
becomes an `orgmap`-adapter screener named after its file stem, invoked as
`script {project}` with cwd set to the project. Drop a script in, it runs.

```toml
[screen]
screener_dirs = ["tooling/screeners"]   # relative to org root, or {org_root}/...
```

Non-executable files (READMEs, fixtures) are ignored; explicit
`[[screen.screener]]` entries win on a name collision; gitleaks still
auto-injects last. This is the ergonomic holoq itself uses — its `modrs-purity`
roof-law screener lives in `tooling/screeners/` and registers with one line.

### The orgmap finding protocol (`adapter = "orgmap"`)

A custom screener prints **NDJSON to stdout**, one finding per line:

```json
{"severity":"warn","file":"src/screen/mod.rs","line":12,"rule":"modrs-purity","message":"roof contains a non-`pub mod` line"}
```

- `severity` ∈ `critical|warn|info` → maps to the tier model + exit-code.
- `file` is project-relative; `line` 1-based; `rule` is the screener's own id;
  `message` is the human string.
- stdout = findings; **exit code**: `0` ran clean, nonzero = the screener
  itself broke (recorded as `screener_errors[name]`, surfaced like the
  gitleaks-bailed note).

Built-in adapters ship for: `orgmap` (native NDJSON) and `gitleaks` (its JSON
report → `Finding`). Any other well-known tool is either another small
adapter or just speaks the native protocol via a 3-line wrapper.

## Refactor checklist (`src/screen.rs`, `src/institution.rs`, `src/main.rs`)

1. `FindingSource::Gitleaks` → `FindingSource::Screener(String)`; update the
   source label in `print_screen_report`.
2. New descriptor `Screener { name, command, args, adapter, optional, severity }`
   + `run_screener(&Screener, project_path, org_root) -> Result<Vec<Finding>, String>`
   dispatching on adapter (gitleaks → temp-report dance; orgmap → placeholder
   substitution → spawn → NDJSON-parse stdout). `resolve_screeners` centralizes
   "registry minus skip minus missing-optional", shared by `run` +
   `active_screeners`.
3. `run_gitleaks` → `adapter_gitleaks(stdout) -> Vec<Finding>`; register a
   default gitleaks `Screener` when config declares none (back-compat).
4. `ScreenConfig` (institution.rs) gains `patterns: Vec<ScreenPattern>` and
   `screeners: Vec<ScreenScreener>` (serde, defaulted).
5. `ScreenOptions.no_gitleaks` → `skip_screeners: Vec<String>`;
   `ProjectFindings.gitleaks_error` → `screener_errors: BTreeMap<String,String>`.
6. CLI: `--skip-screener <name>` (repeatable) + `--list-screeners` (resolves
   the registry without scanning, via `active_screeners`); `--no-gitleaks`
   retained as an alias for `--skip-screener gitleaks`.

## Verification / dogfood

1. After the refactor, the shipped gitleaks screener must produce findings
   **identical** to today (regression-checked on a repo with a planted secret).
2. Prove the native protocol end-to-end with a trivial registered screener
   (a shell script that echoes one NDJSON finding) → it appears in the report
   under `FindingSource::Screener("test")` with the right tier + exit code.
3. Only then does holoq add a real `modrs-purity` screener in *holoq's*
   orgmap.toml, pointing at a holoq script — demonstrating the boundary.

## Boundary (unchanged, reaffirmed)

orgmap core ships: the universal baked patterns (secrets) + the `gitleaks`
adapter + the seam. orgmap core does NOT ship: any org's patterns, any org's
script, any language-specific rule. Those are config + scripts owned by the
adopting org. This is what lets a stranger `git clone orgmap` and screen their
own org without inheriting holoq.
