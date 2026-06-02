namespace OrgmapBinding;

/// <summary>
/// How far a workgroup observes for presence/activity — the C# mirror of Rust
/// <c>orgmap::ObservationMode</c>, a field of <see cref="WorkgroupDefinition"/>.
///
/// Crosses the FFI as a lowercase token (<c>"exact"</c>/<c>"subtree"</c>/
/// <c>"network"</c>); <see cref="Parse"/> maps it, defaulting to
/// <see cref="Subtree"/> for anything unrecognized (matching the Rust parser).
/// </summary>
public enum ObservationMode
{
    /// <summary>Observe only the marker directory itself.</summary>
    Exact,
    /// <summary>Observe the whole subtree beneath the marker (the default).</summary>
    Subtree,
    /// <summary>Observe an explicit set of roots (<c>observation_roots</c>).</summary>
    Network,
}

/// <summary>Wire-token parsing for <see cref="ObservationMode"/>.</summary>
public static class ObservationModeExtensions
{
    /// <summary>
    /// Parse the lowercase wire token serde emits via Rust's
    /// <c>ObservationMode::as_str()</c>. Unknown tokens fall to
    /// <see cref="ObservationMode.Subtree"/>, mirroring the Rust default.
    /// </summary>
    public static ObservationMode Parse(string? token) => token switch
    {
        "exact" => ObservationMode.Exact,
        "network" => ObservationMode.Network,
        _ => ObservationMode.Subtree,
    };
}
