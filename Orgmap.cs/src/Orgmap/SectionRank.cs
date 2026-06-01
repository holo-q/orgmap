namespace OrgmapBinding;

/// <summary>
/// A project's position in an org's <c>[sections]</c> pecking order — the C#
/// face of Rust <c>orgmap::SectionRank</c>, loaded over FFI by
/// <see cref="Orgmap.SectionRanks"/>.
///
/// <see cref="Section"/> is the section block index in <b>file declaration
/// order</b> (the first <c>name = [...]</c> array is section 0 — NOT alphabetical),
/// and <see cref="Project"/> is the project index within that block. This is the
/// visual pecking order HUDs sort by: orgmap-ranked projects float above the
/// unranked rest.
/// </summary>
public readonly record struct SectionRank(int Section, int Project);
