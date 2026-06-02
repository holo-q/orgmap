namespace OrgmapBinding;

/// <summary>
/// A <i>concern</i> whose grouping boundary the org tree can answer for any path
/// — the C# mirror of Rust <c>orgmap::Facet</c>, the argument to
/// <see cref="Orgmap.Boundary"/>.
///
/// The org tree is one structure; different concerns bound at different
/// granularities. Rather than each consumer re-deriving "what root owns my
/// concern here", every concern names its facet and asks
/// <see cref="Orgmap.Boundary"/>.
///
/// Crosses the FFI as a lowercase wire token (<c>"group"</c>/<c>"build"</c>/
/// <c>"presence"</c> via <see cref="ToWireToken"/>); an unknown token defaults to
/// <see cref="Build"/> on the Rust side.
/// </summary>
public enum Facet
{
    /// <summary>
    /// The team room: presence, ticket visibility, identity (color/icon/name).
    /// Boundary = the nearest workgroup marker.
    /// </summary>
    Group,

    /// <summary>
    /// The build-gate unit: who must wait on whom before a build runs. Default
    /// boundary = the nearest structural project (git repo root, else a build
    /// marker), overridable by an explicit <c>[scope] build</c> declaration
    /// (innermost-wins). This is the build-coordination resolver.
    /// </summary>
    Build,

    /// <summary>
    /// Reserved — the future fold of observation-mode/observation-roots. Resolves
    /// as <see cref="Group"/> until observation folds into the same resolver.
    /// </summary>
    Presence,
}

/// <summary>Wire-token marshalling for <see cref="Facet"/> across the orgmap FFI.</summary>
public static class FacetExtensions
{
    /// <summary>
    /// The lowercase wire token the FFI's <c>orgmap_boundary</c> parses
    /// (<c>"group"</c>/<c>"build"</c>/<c>"presence"</c>).
    /// </summary>
    public static string ToWireToken(this Facet facet) => facet switch
    {
        Facet.Group => "group",
        Facet.Presence => "presence",
        _ => "build",
    };
}
