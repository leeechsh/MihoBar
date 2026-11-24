//! Layout tokens and colors (ClashBar's `MenuBarLayoutTokens` and the
//! native colors it draws with: label colors, system blue / green /
//! orange / red), as light and dark pairs.

use nsh_core::prelude::*;

pub const PANEL_WIDTH: f32 = 360.0;
pub const SPACE2: f32 = 2.0;
pub const SPACE4: f32 = 4.0;
pub const SPACE6: f32 = 6.0;
pub const SPACE8: f32 = 8.0;
pub const CORNER_RADIUS: f32 = 6.0;
pub const PANEL_CORNER_RADIUS: f32 = 10.0;
pub const ROW_HEIGHT: f32 = 32.0;
pub const COMPACT_ROW_HEIGHT: f32 = 20.0;

pub mod font {
    pub const STATUS_BAR: f32 = 10.0;
    pub const CAPTION: f32 = 10.0;
    pub const SUBHEADLINE: f32 = 11.0;
    pub const CALLOUT: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const TITLE3: f32 = 15.0;
}

fn c(light: u32, light_alpha: f32, dark: u32, dark_alpha: f32) -> AdaptiveColor {
    AdaptiveColor::new(rgba(light, light_alpha), rgba(dark, dark_alpha))
}

fn rgba(hex: u32, alpha: f32) -> Hsla {
    let r = ((hex >> 16) & 0xff) as f32 / 255.0;
    let g = ((hex >> 8) & 0xff) as f32 / 255.0;
    let b = (hex & 0xff) as f32 / 255.0;
    Rgba { r, g, b, a: alpha }.into()
}

/// `labelColor`.
pub fn primary_label() -> AdaptiveColor {
    c(0x000000, 0.85, 0xffffff, 0.85)
}
/// `secondaryLabelColor`.
pub fn secondary_label() -> AdaptiveColor {
    c(0x000000, 0.50, 0xffffff, 0.55)
}
/// `tertiaryLabelColor`.
pub fn tertiary_label() -> AdaptiveColor {
    c(0x000000, 0.26, 0xffffff, 0.25)
}
/// `systemBlue`.
pub fn info() -> AdaptiveColor {
    c(0x007aff, 1.0, 0x0a84ff, 1.0)
}
/// `systemGreen`.
pub fn positive() -> AdaptiveColor {
    c(0x34c759, 1.0, 0x30d158, 1.0)
}
/// `systemOrange`.
pub fn warning() -> AdaptiveColor {
    c(0xff9500, 1.0, 0xff9f0a, 1.0)
}
/// `systemRed`.
pub fn negative() -> AdaptiveColor {
    c(0xff3b30, 1.0, 0xff453a, 1.0)
}
/// White in both appearances (text on a filled button).
pub fn white() -> AdaptiveColor {
    c(0xffffff, 1.0, 0xffffff, 1.0)
}
/// A neutral fill for selections and hovers.
pub fn fill(opacity: f32) -> AdaptiveColor {
    c(0x000000, opacity, 0xffffff, opacity)
}
/// `separatorColor`.
pub fn separator() -> AdaptiveColor {
    c(0x000000, 0.10, 0xffffff, 0.10)
}
/// The accent tint (`controlAccentColor`) at `opacity`.
pub fn accent(opacity: f32) -> AdaptiveColor {
    c(0x007aff, opacity, 0x0a84ff, opacity)
}
/// `systemTeal`.
pub fn teal() -> AdaptiveColor {
    c(0x30b0c7, 1.0, 0x40c8e0, 1.0)
}
/// `systemIndigo`.
pub fn indigo() -> AdaptiveColor {
    c(0x5856d6, 1.0, 0x5e5ce6, 1.0)
}
/// `systemPurple`.
pub fn purple() -> AdaptiveColor {
    c(0xaf52de, 1.0, 0xbf5af2, 1.0)
}
/// A badge's capsule fill.
pub fn badge_fill() -> AdaptiveColor {
    c(0x000000, 0.06, 0xffffff, 0.10)
}
/// A latency tier's color (ClashBar's `latencyColor`).
pub fn latency(tone: crate::proxy::LatencyTone) -> AdaptiveColor {
    use crate::proxy::LatencyTone::*;
    match tone {
        Unknown => tertiary_label(),
        Timeout => negative(),
        Good => positive(),
        Slow => warning(),
    }
}
