//! The advanced window: rules, connections and logs, out of the menu bar
//! panel. In the app bundle it is a process of its own (`AdvancedLaunch`),
//! started by the panel's and ended as the window closes, so all it used
//! goes back to the system; outside one (development, tests) it is a
//! window of the panel's process. Either way only its current section is
//! mounted, and the lists and streams behind a section live as long as it
//! shows.

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::icons;
use crate::model::{ManagerSection, Model};
use crate::style::{self, font};

pub const WIDTH: f32 = 760.0;
pub const HEIGHT: f32 = 560.0;
const SIDEBAR_WIDTH: f32 = 180.0;

fn section_icon(section: ManagerSection) -> String {
    match section {
        ManagerSection::Rules => icons::shuffle(),
        ManagerSection::Connections => icons::pulse(),
        ManagerSection::Logs => icons::doc_text(),
    }
}

/// The window's content: the sections' sidebar and the current section.
pub fn manager(m: Model) -> impl IntoElement {
    let items: Vec<_> = ManagerSection::ALL.iter().map(|&s| sidebar_item(m.clone(), s)).collect();
    let sidebar = view()
        .w(px(SIDEBAR_WIDTH))
        .px(px(style::SPACE6))
        .pt(px(36.0))
        .gap(px(style::SPACE2))
        .bg(style::sidebar_background())
        .border_r_1()
        .border_color(style::separator())
        .children(items);
    // The top strip moves the window (its title bar is hidden).
    let drag = view().absolute().top(px(0.0)).left(px(0.0)).right(px(0.0)).h(px(30.0)).window_drag(true);
    view().flex_1().flex_row().bg(style::window_background()).child(sidebar).child(content(m)).child(drag)
}

fn sidebar_item(m: Model, section: ManagerSection) -> impl IntoElement {
    let selected = {
        let m = m.clone();
        move || m.manager_section.get() == section
    };
    let (s1, s2) = (selected.clone(), selected.clone());
    let press_m = m.clone();
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .h(px(30.0))
        .px(px(style::SPACE6))
        .rounded(px(7.0))
        .bg(move || if s1() { style::accent(0.13) } else { style::accent(0.0) })
        .hover(|s| s.bg(style::fill(0.05)))
        .transition_colors()
        .duration(120.0)
        .on_press(move || press_m.manager_section.set(section))
        .child(image(ImageSrc::from(section_icon(section))).w(px(14.0)).h(px(14.0)).tint_color(move || {
            if s2() { style::info() } else { style::secondary_label() }
        }))
        .child(
            text(move || m.tr(section.title_key()))
                .text_size(px(font::BODY))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if selected() { style::info() } else { style::primary_label() }),
        )
}

/// The current section under its title. Each section is made as it is
/// chosen and dropped as another is.
fn content(m: Model) -> impl IntoElement {
    let title_m = m.clone();
    let shown = |m: &Model, section: ManagerSection| {
        let m = m.clone();
        move || m.manager_section.get() == section
    };
    let (rules_m, conn_m, logs_m) = (m.clone(), m.clone(), m.clone());
    view()
        .flex_1()
        .min_w(px(0.0))
        .px(px(style::SPACE8 * 2.0))
        .pt(px(36.0))
        .pb(px(style::SPACE8))
        .gap(px(style::SPACE6))
        .child(
            text(move || title_m.tr(title_m.manager_section.get().title_key()))
                .text_size(px(17.0))
                .font_weight(FontWeight::BOLD)
                .text_color(style::primary_label()),
        )
        .child(when(shown(&m, ManagerSection::Rules), move || {
            view().flex_1().child(crate::ui_tabs::rules_tab(rules_m.clone()))
        }))
        .child(when(shown(&m, ManagerSection::Connections), move || {
            view().flex_1().child(crate::ui_tabs::connections_tab(conn_m.clone()))
        }))
        .child(when(shown(&m, ManagerSection::Logs), move || {
            view().flex_1().child(crate::ui_tabs::logs_tab(logs_m.clone()))
        }))
}
