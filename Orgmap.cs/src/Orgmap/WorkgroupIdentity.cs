namespace OrgmapBinding;

/// <summary>
/// The workgroup identity that owns a path — the C# face of Rust
/// <c>orgmap::WorkgroupIdentity</c>, resolved over FFI by
/// <see cref="Orgmap.IdentityForPath"/>.
///
/// A workgroup is a directory-scope identity mark (not a git repo, build root,
/// or activity state). orgmap discovers it by walking upward from a path to the
/// nearest <c>orgmap.toml</c>/<c>workgroup.toml</c>/<c>.hsp/workgroup.toml</c>.
/// </summary>
/// <param name="Root">Canonicalized directory that carries the workgroup marker.</param>
/// <param name="Name">Declared <c>name</c>, or the marker directory's name.</param>
/// <param name="Ansi256">
/// The resolved accent: the declared <c>ansi256</c>/<c>color</c> (with orgmap's
/// theme-balanced perceptual-luminance hex→ansi256 bisection applied for
/// <c>#RRGGBB</c>), else a deterministic name-hash palette pick.
/// </param>
/// <param name="Icon">Declared visual mark, or orgmap's generic fallback glyph.</param>
public sealed record WorkgroupIdentity(string Root, string Name, byte Ansi256, string? Icon);
