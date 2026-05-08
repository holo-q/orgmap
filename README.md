# spaceship-workgroup

Shared workgroup marker protocol for Spaceship tools.

`workgroup.toml` is a directory-scope identity marker. It names an org root,
domain workspace, or project scope; it does not encode git state, build state,
or live agent activity.

```toml
[workgroup]
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
let identity = spaceship_workgroup::identity_for_path(path);
let stack = spaceship_workgroup::discover_workgroup_stack(path);
let marker = spaceship_workgroup::toml_path_for_path(path);
```

Nu callers use the JSON-first CLI:

```nu
let wg = (spacewg identity . | from json)
let stack = (spacewg stack . | from json)
let marker = (spacewg marker . | from json)
```
