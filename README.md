# orgmap

Shared organization/workgroup marker protocol for Holo-Q tools.

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

Nu callers use the JSON-first CLI:

```nu
let wg = (orgmap identity . | from json)
let stack = (orgmap stack . | from json)
let marker = (orgmap marker . | from json)
```
