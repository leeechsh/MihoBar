//! Shared controls in the look of current macOS (System Settings' inset
//! groups and icon tiles, Control Center's round buttons, AppKit's
//! segmented control and switch).

use std::rc::Rc;

use nsh_core::element::{ImageSrc, View};
use nsh_core::prelude::*;

use crate::style::{self, font};

/// An SVG icon tinted `color`, `size` points square.
pub fn icon(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

/// A tab's entrance: NSH's FadeIn, settling 6 points (FadeInUp's 24 read
/// as the panel's bottom bouncing while the window resizes), over 220 ms.
fn entrance_spec() -> MotionSpec {
    MotionPreset::FadeIn.spec().values(MotionChannel::TranslateY, [6.0, 0.0]).timing(|mut t| {
        t.duration_ms = 220.0;
        t
    })
}

/// `content` in a view that stays, replaying the entrance each time `key`
/// changes (a tab switch). One motion for the panel's life: a motion made
/// per mounted tab kept that tab's whole view tree alive after it went
/// (tens of MB per switch). Less motion, when the user asks for it, lands
/// it at once.
pub fn entrance_on<K: PartialEq + 'static>(key: impl Fn() -> K + 'static, content: impl IntoElement) -> View {
    let entrance = motion();
    let last: std::cell::RefCell<Option<K>> = std::cell::RefCell::new(None);
    effect(move || {
        let k = key();
        let changed = last.borrow().as_ref() != Some(&k);
        if changed {
            *last.borrow_mut() = Some(k);
            untracked(|| {
                let _ = entrance.entering(entrance_spec());
            });
        }
    });
    view().bind_motion(&entrance).child(content)
}

/// A popover's entrance (ZoomIn: growing from 90 % as it fades in).
pub fn pop_in(content: impl IntoElement) -> View {
    let entrance = motion().entering(MotionPreset::ZoomIn);
    view().flex_1().bind_motion(&entrance).child(content)
}

/// A tooltip's bubble (AppKit's help tag): small text on a raised,
/// rounded surface. NSH shows a tooltip element as it is, so every
/// tooltip goes through this.
pub fn tip(content: impl IntoReactive<SharedString>) -> View {
    view()
        .max_w(px(260.0))
        .px(px(8.0))
        .py(px(4.0))
        .rounded(px(style::TIP_RADIUS))
        .bg(style::elevated())
        .border_1()
        .border_color(style::card_border())
        .shadow_md()
        .child(text(content).text_size(px(font::SUBHEADLINE)).text_color(style::primary_label()))
}

/// A colored rounded-square icon with a white glyph (System Settings'
/// row icons).
pub fn tile(src: String, color: impl Fn() -> AdaptiveColor + 'static) -> View {
    tile_sized(src, color, style::TILE)
}

pub fn tile_sized(src: String, color: impl Fn() -> AdaptiveColor + 'static, size: f32) -> View {
    view()
        .w(px(size))
        .h(px(size))
        .rounded(px(size * 0.23))
        .items_center()
        .justify_center()
        .bg(move || color())
        .child(icon(src, size * 0.62, style::white))
}

/// A tile color for `key`, the same for the same key (rows whose
/// callers name no color).
pub fn tile_color(key: &str) -> fn() -> AdaptiveColor {
    const PALETTE: [fn() -> AdaptiveColor; 7] =
        [style::info, style::indigo, style::teal, style::positive, style::warning, style::purple, style::pink];
    let hash = key.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32));
    PALETTE[(hash as usize) % PALETTE.len()]
}

/// An inset group (System Settings' rounded section).
pub fn card() -> View {
    view()
        .rounded(px(style::CARD_RADIUS))
        .bg(style::card_fill())
        .border_1()
        .border_color(style::card_border())
        .p(px(style::SPACE4))
}

/// A hairline between rows of a card, inset past their icons.
pub fn divider(inset: f32) -> impl IntoElement {
    view().h(px(0.5)).ml(px(inset)).mr(px(style::SPACE6)).bg(style::separator())
}

/// A section's title above its card, with a count and controls.
pub fn section_title(
    title: impl Fn() -> String + 'static,
    count: Option<Rc<dyn Fn() -> String>>,
    trailing: impl IntoElement,
) -> impl IntoElement {
    let mut row = view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE6))
        .h(px(24.0))
        .child(text(title).text_size(px(font::CALLOUT)).font_weight(FontWeight::SEMIBOLD).text_color(style::secondary_label()));
    if let Some(count) = count {
        row = row.child(count_badge(move || count()));
    }
    row.child(view().flex_1()).child(trailing)
}

pub fn count_badge(content: impl Fn() -> String + 'static) -> impl IntoElement {
    text(content)
        .text_size(px(font::CAPTION))
        .font_weight(FontWeight::SEMIBOLD)
        .tabular_nums()
        .text_color(style::secondary_label())
        .px(px(6.0))
        .py(px(1.0))
        .rounded(px(8.0))
        .bg(style::control_track())
}

/// A borderless round icon button with a tooltip (toolbar style).
pub fn icon_button(
    src: impl Fn() -> String + 'static,
    tone: impl Fn() -> AdaptiveColor + 'static,
    label: impl Fn() -> String + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    let label = Rc::new(label);
    view()
        .w(px(24.0))
        .h(px(24.0))
        .items_center()
        .justify_center()
        .rounded(px(12.0))
        .hover(|s| s.bg(style::control_track()))
        .transition_colors()
        .duration(120.0)
        .tooltip(tip({
            let label = label.clone();
            move || label()
        }))
        .label(move || label())
        .on_press(on_press)
        .child(image(move || ImageSrc::from(src())).w(px(13.0)).h(px(13.0)).tint_color(move || tone()))
}

/// A switch (AppKit's `NSSwitch`, small): accent blue when on.
pub fn switch(on: impl Fn() -> bool + Clone + 'static, disabled: impl Fn() -> bool + Clone + 'static) -> impl IntoElement {
    let on1 = on.clone();
    view()
        .w(px(30.0))
        .h(px(18.0))
        .rounded(px(9.0))
        .bg(move || if on1() { style::info() } else { style::control_track() })
        .border_1()
        .border_color(style::card_border())
        .transition_colors()
        .duration(160.0)
        .opacity(move || if disabled() { 0.45_f32 } else { 1.0_f32 })
        .child(
            view()
                .absolute()
                .top(px(1.0))
                .left(px(1.0))
                // The knob slides (a transform, so it animates).
                .translate_x({
                    let on = on.clone();
                    move || -> DefiniteLength { px(if on() { 12.0 } else { 0.0 }).into() }
                })
                .transition_transform()
                .duration(180.0)
                .w(px(14.0))
                .h(px(14.0))
                .rounded(px(7.0))
                .bg(AdaptiveColor::new(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, Rgba { r: 0.96, g: 0.96, b: 0.96, a: 1.0 }))
                .shadow_sm(),
        )
}

/// A pop-up button's face (AppKit's `NSPopUpButton`): the value and
/// up-down chevrons on a raised surface.
pub fn popup_face(value: impl Fn() -> String + 'static) -> View {
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE4))
        .h(px(22.0))
        .pl(px(8.0))
        .pr(px(5.0))
        .rounded(px(style::CONTROL_RADIUS))
        .bg(style::elevated())
        .border_1()
        .border_color(style::card_border())
        .shadow_xs()
        .child(text(value).truncate().text_size(px(font::CALLOUT)).text_color(style::primary_label()))
        .child(icon(crate::icons::chevron_up_down_small(), 10.0, style::secondary_label))
}

/// A push button (AppKit's rounded bezel): raised, full width of its slot.
pub fn push_button(
    label: impl Fn() -> String + 'static,
    icon_src: impl Fn() -> String + 'static,
    tint: impl Fn() -> AdaptiveColor + 'static,
    on_press: impl Fn() + 'static,
) -> View {
    view()
        .flex_1()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(style::SPACE4))
        .h(px(26.0))
        .rounded(px(style::CONTROL_RADIUS))
        .bg(style::elevated())
        .border_1()
        .border_color(style::card_border())
        .shadow_xs()
        .hover(|s| s.bg(style::control_track()))
        .on_press(on_press)
        .child(image(move || ImageSrc::from(icon_src())).w(px(12.0)).h(px(12.0)).tint_color(move || tint()))
        .child(text(label).truncate().text_size(px(font::CALLOUT)).font_weight(FontWeight::MEDIUM).text_color(style::primary_label()))
}

/// A text field's track (AppKit's rounded search/text field).
pub fn field_track() -> View {
    view().h(px(24.0)).px(px(style::SPACE6)).rounded(px(style::CONTROL_RADIUS)).bg(style::control_track())
}
