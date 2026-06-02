using System.Runtime.CompilerServices;

namespace OrgmapBinding.Interop;

/// Auto-registers the native-library resolver the instant this assembly loads,
/// BEFORE any P/Invoke into <c>liborgmap_ffi</c> fires. The generated
/// <see cref="Native"/> exposes <c>EnsureResolver()</c> "for ModuleInitializer"
/// but the generated output carries no initializer, so without this file the
/// custom resolver (ORGMAP_FFI_DIR / ORGMAP_FFI_PATH / target-walk) never engages
/// and the default probe paths can't find the .so. Hand-written (not in the
/// regenerated Native.cs) so it survives rustbind regeneration. Idempotent +
/// swallowing, so it's harmless for every consumer (babel/bob/hsp).
internal static class ResolverInit
{
    // CA2255: ModuleInitializer is "intended for application code". Here it is the
    // RIGHT tool — a native-interop binding auto-registering its own DllImport
    // resolver the moment it loads, for every consumer, with no consumer opt-in.
    // That is exactly the deterministic-at-load guarantee ModuleInitializer gives.
#pragma warning disable CA2255
    [ModuleInitializer]
    internal static void Init() => Native.EnsureResolver();
#pragma warning restore CA2255
}
