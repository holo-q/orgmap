# orgmap

Shared organization/workgroup marker protocol for Holo-Q tools.

`orgmap` is the institution layer for agents working on a multi-repo computer.
It answers "what org am I inside?", "what repos exist?", and "what work is
active?" from code, not from scattered prompt files. Generators such as
`monoregen` consume this map to emit public artifacts; `org` is the live
operator and agent familiarization surface.

`orgmap.toml` is a directory-scope identity marker. It names an org root,
domain workspace, or project scope; it does not encode git state, build state,
or live agent activity. `workgroup.toml` remains accepted as the legacy local
badge format.

```toml
[orgmap]
name = "repo-os"
level = "domain"
icon = "\U000f0493"
color = "#F74C00"

[observe]
mode = "subtree"
roots = ["../repo-agent"]
```

Rust callers use the library:

```rust
let identity = orgmap::identity_for_path(path);
let stack = orgmap::discover_workgroup_stack(path);
let marker = orgmap::toml_path_for_path(path);
```

Nu callers use the JSON-first `org` CLI:

```nu
let wg = (org identity . | from json)
let stack = (org stack . | from json)
let marker = (org marker . | from json)
```

## CLI

Bare `org` is the operator landing surface for the current institution: it
prints the active org root, the workgroup structure, and the fluent command
surface. `org list` remains the configured-institutions registry.

```sh
org                 # current org structure + command surface
org list            # configured organizations from XDG config and env
org work            # dirty/ahead/no-upstream local project report
org report [NAME]   # project map grouped by workspace
org git [NAME]      # detailed local git-state report
org plug            # Claude/Codex agent plugin carrier report
org intro           # familiarization report: workspaces and short project descriptions
org list --json
org work --json     # machine-readable report for dashboards and agents
org report --json
org git --json
org plug --json
org intro --json
org identity .
org definition .
org stack .
org marker .
```

Config discovery:

1. `--config <path>`
2. `ORGMAP_CONFIG`
3. ancestor `orgmap.toml` containing the full institution schema
4. default org in `$XDG_CONFIG_HOME/orgmap/config/*.toml`
5. legacy `$XDG_CONFIG_HOME/orgmap/config.toml`

User organizations live as one file per org:

```toml
name = "holoq"
root = "~/holoq"
config = "~/holoq/orgmap.toml"
default = true
```

The same fields may be nested under `[org]`. Agent-only sessions can define an
organization without mutating user config:

```sh
ORGMAP_NAME=holoq ORGMAP_ROOT=~/holoq org
ORGMAP_NAME=holoq ORGMAP_CONFIG=~/holoq/orgmap.toml org work
```

Registry shape:

```toml
default = "holoq"

[institutions.holoq]
root = "~/holoq"
config = "~/holoq/orgmap.toml"
```
