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
    [ModuleInitializer]
    internal static void Init() => Native.EnsureResolver();
}
