using System.Text.Json.Serialization;

namespace OrgmapBinding;

/// <summary>
/// Wire shape of <c>orgmap_identity_for_path</c>'s JSON
/// (<c>{"root","name","ansi256","icon"}</c>). Internal — the public surface is
/// the <see cref="WorkgroupIdentity"/> record; this is just the deserialization
/// target before the FFI buffer is freed.
/// </summary>
internal sealed class IdentityJson
{
    public string Root { get; set; } = "";
    public string Name { get; set; } = "";
    public byte Ansi256 { get; set; }
    public string? Icon { get; set; }
}

/// <summary>
/// One row of <c>orgmap_section_ranks</c>'s JSON array
/// (<c>{"name","section","project"}</c>). Internal wire shape feeding the public
/// <c>Dictionary&lt;string, SectionRank&gt;</c>.
/// </summary>
internal sealed class SectionRankJson
{
    public string Name { get; set; } = "";
    public int Section { get; set; }
    public int Project { get; set; }
}

/// <summary>
/// Wire shape of <c>orgmap_boundary</c>'s JSON (<c>{"root":"…"}</c>) — the
/// resolved boundary root for a (path, facet) pair. Internal; the public surface
/// is the bare root string returned by <see cref="Orgmap.Boundary"/>.
/// </summary>
internal sealed class BoundaryJson
{
    public string Root { get; set; } = "";
}

/// <summary>
/// Wire shape of <c>orgmap_definition_for_path</c> / one element of
/// <c>orgmap_discover_workgroup_stack</c>'s array. Mirrors the FFI's
/// <c>WorkgroupDefinitionJson</c> (paths as display strings, level/mode as
/// lowercase tokens, the <c>[scope] build</c> tri-state flattened to
/// <c>build_scope</c>). Internal wire shape feeding the public
/// <see cref="WorkgroupDefinition"/> record.
/// </summary>
internal sealed class WorkgroupDefinitionJson
{
    public string Root { get; set; } = "";
    public string Marker { get; set; } = "";
    public string Name { get; set; } = "";
    public string Level { get; set; } = "";
    public string? Icon { get; set; }
    public string? Color { get; set; }
    public byte? Ansi256 { get; set; }

    // Snake_case wire keys — PropertyNameCaseInsensitive only ignores case, not
    // the underscore, so these need explicit [JsonPropertyName] mappings.
    [JsonPropertyName("observation_mode")]
    public string ObservationMode { get; set; } = "";

    [JsonPropertyName("observation_roots")]
    public string[] ObservationRoots { get; set; } = [];

    [JsonPropertyName("build_scope")]
    public bool? BuildScope { get; set; }
}

/// <summary>
/// Source-generated <see cref="System.Text.Json"/> context for the orgmap FFI
/// wire DTOs. Reflection-free deserialization keeps the library
/// <c>IsAotCompatible</c> — required because consumers (babel, bob) are/will be
/// NativeAOT. <see cref="JsonSourceGenerationOptionsAttribute.PropertyNameCaseInsensitive"/>
/// lets the lowercase JSON keys (<c>root</c>, <c>ansi256</c>) bind to the
/// PascalCase DTO properties without per-property <c>[JsonPropertyName]</c>;
/// genuinely snake_case keys (<c>observation_mode</c>, <c>build_scope</c>) still
/// carry an explicit <c>[JsonPropertyName]</c> since case-insensitivity does not
/// cross the underscore.
/// </summary>
[JsonSourceGenerationOptions(PropertyNameCaseInsensitive = true)]
[JsonSerializable(typeof(IdentityJson))]
[JsonSerializable(typeof(SectionRankJson[]))]
[JsonSerializable(typeof(BoundaryJson))]
[JsonSerializable(typeof(WorkgroupDefinitionJson))]
[JsonSerializable(typeof(WorkgroupDefinitionJson[]))]
internal sealed partial class OrgmapJsonContext : JsonSerializerContext;
