// GENERATED from bindings.json by rustbind — DO NOT EDIT. Regenerate with `rk run rustbind -- gen-cs --config rustbind.toml`. ffi=0.1.0
using System;
using System.Collections.Generic;
using System.Linq;
using System.Runtime.InteropServices;
using System.Reflection;
using System.IO;

namespace OrgmapBinding.Interop;

internal static partial class Native
{
    private const string LibraryName = "orgmap_ffi";

    // Register the resolver as early as possible. Also exposed for ModuleInitializer.
    internal static void EnsureResolver()
    {
        try
        {
            NativeLibrary.SetDllImportResolver(typeof(Native).Assembly, Resolve);
        }
        catch
        {
            // Ignore if already set for this assembly
        }
    }

    private static IntPtr Resolve(string libraryName, Assembly assembly, DllImportSearchPath? searchPath)
    {
        EnsureResolver();
        if (!string.Equals(libraryName, LibraryName, StringComparison.Ordinal))
            return IntPtr.Zero;

        var envDir = Environment.GetEnvironmentVariable("ORGMAP_FFI_DIR");
        if (!string.IsNullOrEmpty(envDir))
        {
            var candidate = Path.Combine(envDir, GetPlatformLibraryFileName(LibraryName));
            if (File.Exists(candidate) && NativeLibrary.TryLoad(candidate, out var handle))
                return handle;
        }
        var envPath = Environment.GetEnvironmentVariable("ORGMAP_FFI_PATH");
        if (!string.IsNullOrEmpty(envPath))
        {
            var full = Path.GetFullPath(envPath);
            if (File.Exists(full) && NativeLibrary.TryLoad(full, out var handle))
                return handle;
        }

        var baseDir = AppContext.BaseDirectory;
        var fileName = GetPlatformLibraryFileName(LibraryName);
        var candidates = new List<string>();
        candidates.Add(Path.Combine(baseDir, fileName));
        candidates.Add(Path.Combine(baseDir, "runtimes", GetRid(), "native", fileName));
        var runtimesDir = Path.Combine(baseDir, "runtimes");
        if (Directory.Exists(runtimesDir))
        {
            foreach (var ridDir in Directory.EnumerateDirectories(runtimesDir))
            {
                var cand = Path.Combine(ridDir, "native", fileName);
                candidates.Add(cand);
            }
        }
        for (int up = 3; up <= 6; up++)
        {
            var ups = new string[up];
            Array.Fill(ups, "..");
            var debugPath = Path.Combine(new[] { baseDir }.Concat(ups).Concat(new[] { "orgmap-ffi", "target", "debug", fileName }).ToArray());
            var releasePath = Path.Combine(new[] { baseDir }.Concat(ups).Concat(new[] { "orgmap-ffi", "target", "release", fileName }).ToArray());
            candidates.Add(debugPath);
            candidates.Add(releasePath);
        }

        foreach (var path in candidates)
        {
            var full = Path.GetFullPath(path);
            if (File.Exists(full) && NativeLibrary.TryLoad(full, out var handle))
                return handle;
        }

        return IntPtr.Zero;
    }

    private static string GetPlatformLibraryFileName(string baseName)
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return baseName + ".dll";
        if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            return "lib" + baseName + ".dylib";
        return "lib" + baseName + ".so";
    }

    private static string GetRid()
    {
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
            return RuntimeInformation.OSArchitecture == Architecture.Arm64 ? "win-arm64" : (Environment.Is64BitProcess ? "win-x64" : "win-x86");
        if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
            return RuntimeInformation.OSArchitecture == Architecture.Arm64 ? "osx-arm64" : "osx-x64";
        if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
            return RuntimeInformation.OSArchitecture == Architecture.Arm64 ? "linux-arm64" : "linux-x64";
        return "linux-x64";
    }

    // ===== Enums (named integer constants) =====

    // ===== Bitflags (named integer constants) =====

    // ===== Value structs (repr(C); field order is the memory layout) =====

    // ===== Functions (4 DllImports, emitted in manifest order) =====
    /// <summary>
    /// Parse a color token (bare ANSI index, `#RRGGBB` hex, or named ANSI color) to
    /// an ansi256 index. Returns `true` and writes the index to `out_ansi` when the
    /// token resolves; returns `false` (leaving `out_ansi` untouched) otherwise.
    /// 
    /// Wraps [`orgmap::color_text_to_ansi256`] — including the theme-balanced
    /// perceptual-luminance hex→ansi256 bisection that babel's C# port could not
    /// replicate.
    /// 
    /// # Safety
    /// `text` must be a valid NUL-terminated UTF-8 C string; `out_ansi` must be a
    /// valid, writable `*mut u8`.
    /// </summary>
    [DllImport(LibraryName, EntryPoint = "orgmap_color_text_to_ansi256", CallingConvention = CallingConvention.Cdecl)]
    [return: MarshalAs(UnmanagedType.I1)]
    internal static extern bool OrgmapColorTextToAnsi256(IntPtr text, IntPtr outAnsi);
    /// <summary>
    /// Resolve a path to its owning workgroup identity, returned as JSON
    /// (`{"root","name","ansi256","icon"}`) in a freshly-allocated C string, or
    /// **null** when no workgroup marker encloses the path.
    /// 
    /// Wraps [`orgmap::identity_for_path`]. Free the result with
    /// [`orgmap_string_free`].
    /// 
    /// # Safety
    /// `path` must be a valid NUL-terminated UTF-8 C string.
    /// </summary>
    [DllImport(LibraryName, EntryPoint = "orgmap_identity_for_path", CallingConvention = CallingConvention.Cdecl)]
    internal static extern IntPtr OrgmapIdentityForPath(IntPtr path);
    /// <summary>
    /// Read an `orgmap.toml`'s `[sections]` table into a JSON **array** of
    /// `{"name","section","project"}` rows (section/project in file declaration
    /// order), in a freshly-allocated C string. Never null for a valid call: a
    /// missing/unreadable/sections-less file yields `"[]"`.
    /// 
    /// Wraps [`orgmap::section_ranks`]. Free the result with [`orgmap_string_free`].
    /// An array (not an object) is used so the C# side can build its dictionary
    /// without imposing JSON-object key ordering semantics.
    /// 
    /// # Safety
    /// `orgmap_toml_path` must be a valid NUL-terminated UTF-8 C string.
    /// </summary>
    [DllImport(LibraryName, EntryPoint = "orgmap_section_ranks", CallingConvention = CallingConvention.Cdecl)]
    internal static extern IntPtr OrgmapSectionRanks(IntPtr orgmapTomlPath);
    /// <summary>
    /// Free a C string previously returned by an `orgmap_*` function. Null-safe.
    /// 
    /// # Safety
    /// `s` must be null or a pointer returned by this library's `into_c_char` (i.e.
    /// `CString::into_raw`), freed exactly once.
    /// </summary>
    [DllImport(LibraryName, EntryPoint = "orgmap_string_free", CallingConvention = CallingConvention.Cdecl)]
    internal static extern void OrgmapStringFree(IntPtr s);
}
