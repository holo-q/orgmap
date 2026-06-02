namespace OrgmapBinding;

/// <summary>
/// The full parsed workgroup node that owns a path — the C# face of Rust
/// <c>orgmap::WorkgroupDefinition</c>, resolved over FFI by
/// <see cref="Orgmap.DefinitionForPath"/> (the innermost enclosing node) and
/// <see cref="Orgmap.DiscoverWorkgroupStack"/> (the full ancestor stack).
///
/// This is the richer form behind <see cref="WorkgroupIdentity"/>: identity
/// (<see cref="Name"/>/<see cref="Icon"/>/<see cref="Color"/>/<see cref="Ansi256"/>)
/// plus the structural facts (<see cref="Level"/>, <see cref="ObservationMode"/>,
/// <see cref="ObservationRoots"/>) and the build-gate declaration
/// (<see cref="BuildScope"/>) that <see cref="Orgmap.Boundary"/> resolves against.
/// </summary>
/// <param name="Root">Canonicalized directory that carries the workgroup marker.</param>
/// <param name="Marker">Canonicalized path to the marker file itself.</param>
/// <param name="Name">Declared <c>name</c>, or the marker directory's name.</param>
/// <param name="Level">
/// Declared <c>level</c> as a lowercase token — <c>"umbrella"</c>/<c>"domain"</c>/
/// <c>"project"</c>, or an arbitrary custom string (Rust's
/// <c>WorkgroupLevel::Custom</c>), so this stays a <see cref="string"/> rather
/// than a closed enum.
/// </param>
/// <param name="Icon">Declared visual mark, or <c>null</c> when unset (no fallback applied — that is <see cref="WorkgroupIdentity"/>'s job).</param>
/// <param name="Color">Declared raw <c>color</c> token, or <c>null</c>.</param>
/// <param name="Ansi256">Declared/derived <c>ansi256</c> accent, or <c>null</c> when unset.</param>
/// <param name="ObservationMode">How far this workgroup observes for presence/activity.</param>
/// <param name="ObservationRoots">Explicit observation roots (used when <see cref="ObservationMode"/> is <see cref="OrgmapBinding.ObservationMode.Network"/>).</param>
/// <param name="BuildScope">
/// The explicit <c>[scope] build</c> tri-state: <c>true</c> = this node IS the
/// build boundary (swallows its subtree), <c>false</c> = explicitly NOT (defer
/// upward), <c>null</c> = no opinion (structural project-marker status stands).
/// </param>
public sealed record WorkgroupDefinition(
    string Root,
    string Marker,
    string Name,
    string Level,
    string? Icon,
    string? Color,
    byte? Ansi256,
    ObservationMode ObservationMode,
    IReadOnlyList<string> ObservationRoots,
    bool? BuildScope);
