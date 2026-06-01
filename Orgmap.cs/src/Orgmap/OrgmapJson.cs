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
/// Source-generated <see cref="System.Text.Json"/> context for the orgmap FFI
/// wire DTOs. Reflection-free deserialization keeps the library
/// <c>IsAotCompatible</c> — required because consumers (babel, bob) are/will be
/// NativeAOT. <see cref="JsonSourceGenerationOptionsAttribute.PropertyNameCaseInsensitive"/>
/// lets the lowercase JSON keys (<c>root</c>, <c>ansi256</c>) bind to the
/// PascalCase DTO properties without per-property <c>[JsonPropertyName]</c>.
/// </summary>
[JsonSourceGenerationOptions(PropertyNameCaseInsensitive = true)]
[JsonSerializable(typeof(IdentityJson))]
[JsonSerializable(typeof(SectionRankJson[]))]
internal sealed partial class OrgmapJsonContext : JsonSerializerContext;
