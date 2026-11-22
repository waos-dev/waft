//! Flutter counterpart: dart:ui `painting.dart` (`Clip`).

/// Different ways to clip content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clip {
    /// No clip at all.
    None,

    /// Clip, but do not apply anti-aliasing.
    HardEdge,

    /// Clip with anti-aliasing.
    AntiAlias,

    /// Clip with anti-aliasing plus a save layer immediately after the clip.
    /// This is much slower.
    AntiAliasWithSaveLayer,
}

/// Dart's `Clip.toString`: the enum's name with its class (`Clip.hardEdge`).
impl std::fmt::Display for Clip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Clip::None => "none",
            Clip::HardEdge => "hardEdge",
            Clip::AntiAlias => "antiAlias",
            Clip::AntiAliasWithSaveLayer => "antiAliasWithSaveLayer",
        };
        write!(f, "Clip.{name}")
    }
}
