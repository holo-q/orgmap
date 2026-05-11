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

Bare `org` defaults to `org work`.

```sh
org                 # current operational report
org work            # dirty/ahead/no-upstream local project report
org intro           # familiarization report: workspaces and short project descriptions
org work --json     # machine-readable report for dashboards and agents
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
4. `$XDG_CONFIG_HOME/orgmap/config.toml`

Registry shape:

```toml
default = "holoq"

[institutions.holoq]
root = "~/holoq"
config = "~/holoq/orgmap.toml"
```
