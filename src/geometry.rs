//! Where the panel and its attached popovers go (ClashBar's
//! `PanelGeometry`). Screen coordinates in points, `y` growing down.

use nsh_core::prelude::*;

/// A rectangle `x, y, width, height`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn from_bounds(b: Bounds<Points>) -> Self {
        Self { x: b.origin.x.0, y: b.origin.y.0, w: b.size.width.0, h: b.size.height.0 }
    }
    pub fn max_x(&self) -> f32 {
        self.x + self.w
    }
    pub fn max_y(&self) -> f32 {
        self.y + self.h
    }
    pub fn contains_x(&self, x: f32) -> bool {
        x >= self.x && x <= self.max_x()
    }
}

/// The visible part of the display showing `x` (else the primary one).
pub fn visible_frame_at(x: f32) -> Rect {
    let displays = nsh_core::element::desktop::displays();
    displays
        .iter()
        .map(|d| (Rect::from_bounds(d.bounds), Rect::from_bounds(d.visible_bounds), d.primary))
        .find(|(b, _, _)| b.contains_x(x))
        .or_else(|| {
            displays
                .iter()
                .find(|d| d.primary)
                .map(|d| (Rect::from_bounds(d.bounds), Rect::from_bounds(d.visible_bounds), true))
        })
        .map(|(_, visible, _)| visible)
        .unwrap_or(Rect { x: 0.0, y: 25.0, w: 1440.0, h: 875.0 })
}

/// The panel's origin: centered under the menu bar icon, kept on screen,
/// its top `spacing` below the menu bar.
pub fn panel_origin(icon: Rect, panel_w: f32, spacing: f32) -> (f32, f32) {
    let visible = visible_frame_at(icon.x + icon.w / 2.0);
    let padding = 8.0;
    let centered = icon.x + icon.w / 2.0 - panel_w / 2.0;
    let x = centered.clamp(visible.x + padding, (visible.max_x() - padding - panel_w).max(visible.x + padding));
    let y = icon.max_y().max(visible.y) + spacing;
    (x, y)
}

/// An attached popover's frame: beside `host` (right when it fits, else
/// left, overlapping its edge by a point), centered on `anchor_mid_y`,
/// kept on screen.
pub fn attached_frame(host: Rect, anchor_mid_y: f32, w: f32, h: f32) -> Rect {
    let visible = visible_frame_at(host.x + host.w / 2.0);
    let padding = 8.0;
    let edge_overlap = 1.0;
    let h = h.min(visible.h - padding * 2.0).max(40.0);
    let right_available = (visible.max_x() - host.max_x()).max(0.0);
    let left_available = (host.x - visible.x).max(0.0);
    let right = right_available >= w || right_available >= left_available;
    let w = w.min(if right { right_available } else { left_available }).max(1.0);
    let raw_x = if right { host.max_x() - edge_overlap } else { host.x - w + edge_overlap };
    let x = raw_x.clamp(visible.x + padding, (visible.max_x() - padding - w).max(visible.x + padding));
    let desired_y = anchor_mid_y - h / 2.0;
    let y = desired_y.clamp(visible.y + padding, (visible.max_y() - padding - h).max(visible.y + padding));
    Rect { x, y, w, h }
}
