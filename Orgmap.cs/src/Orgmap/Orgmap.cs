using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using OrgmapBinding.Interop;

namespace OrgmapBinding;

/// <summary>
/// The ergonomic C# face of the orgmap workgroup-identity protocol — a thin
/// hand-written layer over the rustbind-generated <see cref="Native"/> interop
/// (<c>Interop/Native.cs</c>). The Rust <c>orgmap</c> crate is the single source
/// of truth; this just marshals strings/JSON across the C ABI and frees the
/// returned buffers.
///
/// <para>Replaces three hand-rolled orgmap reimplementations in babel
/// (project-metrics identity + hex→ansi256, hud <c>[sections]</c> rank loader,
/// world workgroup-paint stub). The big win over those ports: the
/// <c>#RRGGBB</c> path now runs orgmap's real theme-balanced
/// perceptual-luminance bisection instead of babel's cube-quantizer fallback —
/// the conversion is exact, not approximate.</para>
///
/// <para>AOT-clean: JSON crosses via the source-generated
/// <see cref="OrgmapJsonContext"/>; string marshalling is manual
/// (<see cref="Marshal"/>) so there is no reflection-based interop.</para>
/// </summary>
public static class Orgmap
{
    /// <summary>
    /// Resolve a path to its owning workgroup identity, or <c>null</c> when no
    /// workgroup marker (<c>orgmap.toml</c>/<c>workgroup.toml</c>/<c>.hsp/workgroup.toml</c>)
    /// encloses it. Wraps <c>orgmap_identity_for_path</c>.
    /// </summary>
    public static WorkgroupIdentity? IdentityForPath(string path)
    {
        IntPtr pathPtr = Utf8.Alloc(path);
        try
        {
            IntPtr json = Native.OrgmapIdentityForPath(pathPtr);
            if (json == IntPtr.Zero)
                return null;
            try
            {
                string text = Utf8.ReadAndOwn(json);
                IdentityJson? dto = JsonSerializer.Deserialize(text, OrgmapJsonContext.Default.IdentityJson);
                return dto is null
                    ? null
                    : new WorkgroupIdentity(dto.Root, dto.Name, dto.Ansi256, dto.Icon);
            }
            finally
            {
                Native.OrgmapStringFree(json);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    /// <summary>
    /// The single accent value babel's project-metrics attached per row: the
    /// owning workgroup's ansi256, or <c>null</c> when no marker encloses the
    /// path. Convenience over <see cref="IdentityForPath"/>.
    /// </summary>
    public static byte? Ansi256ForProject(string path) => IdentityForPath(path)?.Ansi256;

    /// <summary>
    /// Parse a color token (bare ANSI index, <c>#RRGGBB</c> hex, or named ANSI
    /// color) to an ansi256 index, or <c>null</c> when it does not resolve. Wraps
    /// <c>orgmap_color_text_to_ansi256</c> — the hex path is orgmap's exact
    /// theme-balanced perceptual-luminance bisection.
    /// </summary>
    public static byte? ColorTextToAnsi256(string text)
    {
        IntPtr textPtr = Utf8.Alloc(text);
        try
        {
            byte ansi = 0;
            unsafe
            {
                bool ok = Native.OrgmapColorTextToAnsi256(textPtr, (IntPtr)(&ansi));
                return ok ? ansi : null;
            }
        }
        finally
        {
            Marshal.FreeHGlobal(textPtr);
        }
    }

    /// <summary>
    /// Read an <c>orgmap.toml</c>'s <c>[sections]</c> table into a project →
    /// <see cref="SectionRank"/> map (section/project in file declaration order).
    /// A null/empty path resolves the default location
    /// (<see cref="DefaultOrgmapTomlPath"/>); a missing/unreadable/sections-less
    /// file yields an empty map. Wraps <c>orgmap_section_ranks</c>.
    /// </summary>
    public static Dictionary<string, SectionRank> SectionRanks(string? orgmapTomlPath = null)
    {
        string path = string.IsNullOrEmpty(orgmapTomlPath) ? DefaultOrgmapTomlPath() : orgmapTomlPath;
        IntPtr pathPtr = Utf8.Alloc(path);
        try
        {
            IntPtr json = Native.OrgmapSectionRanks(pathPtr);
            if (json == IntPtr.Zero)
                return [];
            try
            {
                string text = Utf8.ReadAndOwn(json);
                SectionRankJson[]? rows = JsonSerializer.Deserialize(text, OrgmapJsonContext.Default.SectionRankJsonArray);
                Dictionary<string, SectionRank> ranks = [];
                if (rows is not null)
                    foreach (SectionRankJson row in rows)
                        ranks[row.Name] = new SectionRank(row.Section, row.Project);
                return ranks;
            }
            finally
            {
                Native.OrgmapStringFree(json);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    /// <summary>
    /// Resolve the boundary root that owns <paramref name="facet"/> at
    /// <paramref name="path"/> — the one entry point for "what root owns my
    /// concern here" instead of hand-rolling a tree walk. For
    /// <see cref="Facet.Build"/> this is the build-gate / "who-waits-on-whom"
    /// unit (nearest git repo root, else a build marker, else an explicit
    /// <c>[scope] build</c> boundary). Wraps <c>orgmap_boundary</c>.
    ///
    /// <para>Never returns <c>null</c> for a non-null <paramref name="path"/>:
    /// orgmap always yields a root (the path itself is the floor). Returns
    /// <c>path</c> verbatim only in the degenerate case where the FFI buffer
    /// could not be read.</para>
    /// </summary>
    public static string Boundary(string path, Facet facet)
    {
        IntPtr pathPtr = Utf8.Alloc(path);
        IntPtr facetPtr = Utf8.Alloc(facet.ToWireToken());
        try
        {
            IntPtr json = Native.OrgmapBoundary(pathPtr, facetPtr);
            if (json == IntPtr.Zero)
                return path;
            try
            {
                string text = Utf8.ReadAndOwn(json);
                BoundaryJson? dto = JsonSerializer.Deserialize(text, OrgmapJsonContext.Default.BoundaryJson);
                return dto?.Root ?? path;
            }
            finally
            {
                Native.OrgmapStringFree(json);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
            Marshal.FreeHGlobal(facetPtr);
        }
    }

    /// <summary>
    /// Resolve a path to its owning workgroup definition (the full parsed node
    /// behind <see cref="IdentityForPath"/>), or <c>null</c> when no workgroup
    /// marker encloses it. Wraps <c>orgmap_definition_for_path</c>.
    /// </summary>
    public static WorkgroupDefinition? DefinitionForPath(string path)
    {
        IntPtr pathPtr = Utf8.Alloc(path);
        try
        {
            IntPtr json = Native.OrgmapDefinitionForPath(pathPtr);
            if (json == IntPtr.Zero)
                return null;
            try
            {
                string text = Utf8.ReadAndOwn(json);
                WorkgroupDefinitionJson? dto = JsonSerializer.Deserialize(text, OrgmapJsonContext.Default.WorkgroupDefinitionJson);
                return dto is null ? null : FromJson(dto);
            }
            finally
            {
                Native.OrgmapStringFree(json);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    /// <summary>
    /// Resolve the full ancestor stack of workgroup definitions enclosing
    /// <paramref name="path"/>, ordered outermost → innermost. Empty when no
    /// markers enclose the path. Wraps <c>orgmap_discover_workgroup_stack</c>.
    /// </summary>
    public static List<WorkgroupDefinition> DiscoverWorkgroupStack(string path)
    {
        IntPtr pathPtr = Utf8.Alloc(path);
        try
        {
            IntPtr json = Native.OrgmapDiscoverWorkgroupStack(pathPtr);
            if (json == IntPtr.Zero)
                return [];
            try
            {
                string text = Utf8.ReadAndOwn(json);
                WorkgroupDefinitionJson[]? rows = JsonSerializer.Deserialize(text, OrgmapJsonContext.Default.WorkgroupDefinitionJsonArray);
                List<WorkgroupDefinition> stack = [];
                if (rows is not null)
                    foreach (WorkgroupDefinitionJson row in rows)
                        stack.Add(FromJson(row));
                return stack;
            }
            finally
            {
                Native.OrgmapStringFree(json);
            }
        }
        finally
        {
            Marshal.FreeHGlobal(pathPtr);
        }
    }

    /// <summary>Build the public <see cref="WorkgroupDefinition"/> record from its wire DTO.</summary>
    private static WorkgroupDefinition FromJson(WorkgroupDefinitionJson dto) =>
        new(
            dto.Root,
            dto.Marker,
            dto.Name,
            dto.Level,
            dto.Icon,
            dto.Color,
            dto.Ansi256,
            ObservationModeExtensions.Parse(dto.ObservationMode),
            dto.ObservationRoots,
            dto.BuildScope);

    /// <summary>
    /// Resolve the orgmap.toml path the same way babel's HUD did:
    /// <c>$BABEL_ORGMAP_CONFIG</c> → <c>$BOB_ORGMAP_CONFIG</c> (both
    /// <c>~</c>-expanded) → <c>$HOME/holoq/orgmap.toml</c> → bare
    /// <c>orgmap.toml</c>. Kept here so consumers don't re-derive it.
    /// </summary>
    public static string DefaultOrgmapTomlPath()
    {
        string? configured = Environment.GetEnvironmentVariable("BABEL_ORGMAP_CONFIG")
            ?? Environment.GetEnvironmentVariable("BOB_ORGMAP_CONFIG");
        if (configured is not null)
            return ExpandHome(configured);

        string? home = Environment.GetEnvironmentVariable("HOME");
        return home is null
            ? "orgmap.toml"
            : Path.Combine(home, "holoq", "orgmap.toml");
    }

    /// <summary>Expand a leading <c>~</c> / <c>~/</c> against <c>$HOME</c>.</summary>
    private static string ExpandHome(string path)
    {
        string? home = Environment.GetEnvironmentVariable("HOME");
        if (path == "~")
            return home ?? path;
        if (path.StartsWith("~/", StringComparison.Ordinal))
            return home is null ? path : Path.Combine(home, path[2..]);
        return path;
    }
}

/// <summary>
/// Manual UTF-8 C-string marshalling helpers for the orgmap FFI boundary. The
/// generated <see cref="Native"/> externs take/return <c>IntPtr</c> (rustbind's
/// uniform pointer policy), so the ergonomic layer owns the NUL-terminated UTF-8
/// allocation and the free of orgmap's returned buffers — done by hand to stay
/// reflection-free for NativeAOT consumers.
/// </summary>
internal static class Utf8
{
    /// <summary>Allocate a NUL-terminated UTF-8 copy of <paramref name="s"/>; free with <see cref="Marshal.FreeHGlobal"/>.</summary>
    internal static IntPtr Alloc(string s)
    {
        byte[] bytes = Encoding.UTF8.GetBytes(s);
        IntPtr ptr = Marshal.AllocHGlobal(bytes.Length + 1);
        Marshal.Copy(bytes, 0, ptr, bytes.Length);
        Marshal.WriteByte(ptr, bytes.Length, 0);
        return ptr;
    }

    /// <summary>
    /// Read a NUL-terminated UTF-8 string from an orgmap-returned pointer. Does
    /// NOT free it — the caller frees via <c>orgmap_string_free</c> (the buffer
    /// is Rust-owned, so it must cross back to Rust to be released).
    /// </summary>
    internal static string ReadAndOwn(IntPtr ptr) =>
        Marshal.PtrToStringUTF8(ptr) ?? "";
}
