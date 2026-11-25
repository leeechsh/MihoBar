//! Layout tokens and colors (ClashBar's `MenuBarLayoutTokens` and the
//! native colors it draws with: label colors, system blue / green /
//! orange / red), as light and dark pairs.

use nsh_core::prelude::*;

pub const PANEL_WIDTH: f32 = 340.0;
pub const SPACE2: f32 = 2.0;
pub const SPACE4: f32 = 4.0;
pub const SPACE6: f32 = 6.0;
pub const SPACE8: f32 = 8.0;
/// The corners, concentric as macOS draws them (each one the corner
/// around it less the padding between): the panel and its popover 18;
/// their cards 10 (8 in); a card's rows 6 (4 in); controls 6.
pub const WINDOW_RADIUS: f32 = 18.0;

/// A frameless window's surface (the panel, its popover): the popover
/// material behind the window, rounded at `WINDOW_RADIUS` with a hairline,
/// as macOS draws a menu bar extra (the window itself is transparent).
pub fn window_surface(surface: nsh_core::element::View) -> nsh_core::element::View {
    use nsh_core::prelude::*;
    let mut material = nsh_core::MacOsBackdrop::default();
    material.material = nsh_core::MacOsBackdropMaterial::Popover;
    material.blending = nsh_core::MacOsBackdropBlending::BehindWindow;
    material.state = nsh_core::MacOsBackdropState::Active;
    surface
        .rounded(px(WINDOW_RADIUS))
        .overflow_hidden()
        .macos_backdrop(Some(material))
        .border_1()
        .border_color(card_border())
}
pub const ROW_RADIUS: f32 = 6.0;
pub const CONTROL_RADIUS: f32 = 6.0;
pub const TIP_RADIUS: f32 = 5.0;
/// A toolbar item, straight in the window (8 in).
pub const TAB_RADIUS: f32 = 10.0;
pub const CORNER_RADIUS: f32 = 6.0;
/// A grouped card's corners (System Settings' inset groups).
pub const CARD_RADIUS: f32 = 10.0;
/// A colored icon tile's side (System Settings' row icons).
pub const TILE: f32 = 22.0;
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
/// The panel's white material: laid over the popover backdrop (white in
/// light, the menu gray in dark), as a native menu's.
pub fn menu_background() -> AdaptiveColor {
    c(0xffffff, 0.82, 0x2a2a2c, 0.55)
}
/// A normal window's opaque background (`windowBackgroundColor`): the
/// advanced window.
pub fn window_background() -> AdaptiveColor {
    c(0xffffff, 1.0, 0x1e1e1e, 1.0)
}
/// The advanced window's sidebar (opaque, a shade off the window).
pub fn sidebar_background() -> AdaptiveColor {
    c(0xf5f5f7, 1.0, 0x2a2a2c, 1.0)
}
/// An inset group's fill on the white panel.
pub fn card_fill() -> AdaptiveColor {
    c(0x000000, 0.035, 0xffffff, 0.065)
}
/// An inset group's hairline border.
pub fn card_border() -> AdaptiveColor {
    c(0x000000, 0.07, 0xffffff, 0.08)
}
/// A segmented control's or a field's track.
pub fn control_track() -> AdaptiveColor {
    c(0x000000, 0.06, 0xffffff, 0.09)
}
/// A raised control surface: a segment's thumb, a pop-up button.
pub fn elevated() -> AdaptiveColor {
    c(0xffffff, 1.0, 0x6b6b70, 1.0)
}
/// `systemPink`.
pub fn pink() -> AdaptiveColor {
    c(0xff2d55, 1.0, 0xff375f, 1.0)
}
/// `systemGray`.
pub fn gray() -> AdaptiveColor {
    c(0x8e8e93, 1.0, 0x98989d, 1.0)
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

/// The advanced window's palette: zashboard's light and dark themes.
pub mod z {
    use nsh_core::prelude::AdaptiveColor;

    fn c(light: u32, light_alpha: f32, dark: u32, dark_alpha: f32) -> AdaptiveColor {
        super::c(light, light_alpha, dark, dark_alpha)
    }

    /// `base-200`: the page under the cards.
    pub fn page() -> AdaptiveColor {
        c(0xf5f5f7, 1.0, 0x000000, 1.0)
    }
    /// `base-200` at `alpha` (toolbars, table headers).
    pub fn base200(alpha: f32) -> AdaptiveColor {
        c(0xf5f5f7, alpha, 0x000000, alpha)
    }
    /// `base-100`: cards.
    pub fn card() -> AdaptiveColor {
        c(0xffffff, 1.0, 0x1d1d1f, 1.0)
    }
    /// `base-300` at `alpha` (borders).
    pub fn base300(alpha: f32) -> AdaptiveColor {
        c(0xd2d2d7, alpha, 0x424245, alpha)
    }
    /// `base-content` at `alpha` (text; 0.6 for muted).
    pub fn text(alpha: f32) -> AdaptiveColor {
        c(0x1d1d1f, alpha, 0xf5f5f7, alpha)
    }
    pub fn primary(alpha: f32) -> AdaptiveColor {
        c(0x454a51, alpha, 0xf5f5f7, alpha)
    }
    pub fn primary_content() -> AdaptiveColor {
        c(0xffffff, 1.0, 0x1d1d1f, 1.0)
    }
    pub fn error() -> AdaptiveColor {
        c(0xff3b30, 1.0, 0xff453a, 1.0)
    }
    pub fn success() -> AdaptiveColor {
        c(0x34c759, 1.0, 0x30d158, 1.0)
    }
    pub fn info() -> AdaptiveColor {
        c(0x5ac8fa, 1.0, 0x64d2ff, 1.0)
    }
    pub fn warning() -> AdaptiveColor {
        c(0xff9500, 1.0, 0xff9f0a, 1.0)
    }
}

