//! The settings tab (ClashBar's `SystemTabView`): basic settings, core
//! settings, proxy ports and maintenance.

use std::rc::Rc;

use nsh_core::element::desktop::{MenuItem, popup_menu};
use nsh_core::element::{ImageSrc, input};
use nsh_core::prelude::*;

use crate::icons;
use crate::model::{Model, run_root};
use crate::settings::{AppLanguage, AppearanceMode, StatusBarDisplayMode};
use crate::settings_core::{self, Feedback, TaskState};
use crate::style::{self, font};

fn icon_view(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

/// A card's header (ClashBar's `settingsCardHeader`).
fn card_header(icon: String, title: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE4))
        .pt(px(style::SPACE6))
        .pb(px(style::SPACE4))
        .child(icon_view(icon, 11.0, style::tertiary_label))
        .child(
            text(move || title().to_uppercase())
                .text_size(px(font::BODY))
                .font_weight(FontWeight::BOLD)
                .text_color(style::tertiary_label()),
        )
}

fn row_label(icon: String, title: impl Fn() -> String + 'static) -> impl IntoElement {
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .child(view().w(px(16.0)).items_center().child(icon_view(icon, 12.0, style::secondary_label)))
        .child(text(title).text_size(px(font::BODY)).text_color(style::primary_label()))
}

/// A switch (a native toggle's look).
pub(crate) fn switch(on: impl Fn() -> bool + Clone + 'static, disabled: impl Fn() -> bool + Clone + 'static) -> impl IntoElement {
    let on1 = on.clone();
    view()
        .w(px(28.0))
        .h(px(16.0))
        .rounded(px(8.0))
        .bg(move || if on1() { style::positive() } else { style::fill(0.15) })
        .opacity(move || if disabled() { 0.5_f32 } else { 1.0_f32 })
        .child(
            view()
                .absolute()
                .top(px(2.0))
                .left({
                    let on = on.clone();
                    move || px(if on() { 14.0 } else { 2.0 })
                })
                .w(px(12.0))
                .h(px(12.0))
                .rounded(px(6.0))
                .bg(AdaptiveColor::new(Rgba { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }, Rgba { r: 0.95, g: 0.95, b: 0.95, a: 1.0 })),
        )
}

fn toggle_row(
    icon: String,
    title: impl Fn() -> String + 'static,
    on: impl Fn() -> bool + Clone + 'static,
    disabled: impl Fn() -> bool + Clone + 'static,
    set: impl Fn(bool) + 'static,
) -> impl IntoElement {
    let (on1, d1) = (on.clone(), disabled.clone());
    view()
        .flex_row()
        .items_center()
        .px(px(style::SPACE4))
        .py(px(style::SPACE6))
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.04)))
        .on_press(move || {
            if !d1() {
                set(!on1());
            }
        })
        .child(row_label(icon, title))
        .child(view().flex_1())
        .child(switch(on, disabled))
}

/// A row whose value is chosen from a menu (ClashBar's `settingsSelectionRow`).
fn selection_row<T: Copy + PartialEq + 'static>(
    m: &Model,
    icon: String,
    title_key: &'static str,
    options: &'static [T],
    current: impl Fn() -> T + 'static,
    label: impl Fn(&Model, T) -> String + Clone + 'static,
    on_select: impl Fn(T) + Clone + 'static,
) -> impl IntoElement {
    let current = Rc::new(current);
    let title_m = m.clone();
    let value_m = m.clone();
    let menu_m = m.clone();
    let (value_label, menu_label) = (label.clone(), label);
    let value_current = current.clone();
    view()
        .flex_row()
        .items_center()
        .px(px(style::SPACE4))
        .py(px(style::SPACE4))
        .child(row_label(icon, move || title_m.tr(title_key)))
        .child(view().flex_1())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE2))
                .px(px(style::SPACE6))
                .py(px(style::SPACE2))
                .rounded(px(style::CORNER_RADIUS))
                .border_1()
                .border_color(style::separator())
                .hover(|s| s.bg(style::fill(0.06)))
                .on_press(move || {
                    let selected = current();
                    let items = options
                        .iter()
                        .map(|&o| {
                            let on_select = on_select.clone();
                            MenuItem::new(menu_label(&menu_m, o)).checked(o == selected).on_select(move || on_select(o))
                        })
                        .collect();
                    let _ = popup_menu(items);
                })
                .child(
                    text(move || value_label(&value_m, value_current()))
                        .text_size(px(font::CALLOUT))
                        .text_color(style::secondary_label()),
                )
                .child(icon_view(icons::chevron_right(), 8.0, style::tertiary_label)),
        )
}

fn port_row(m: &Model, key: &'static str, label_key: &'static str, icon: String) -> impl IntoElement {
    let (title_m, value_m, edit_m) = (m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .child(row_label(icon, move || title_m.tr(label_key)))
        .child(view().flex_1())
        .child(
            input()
                .w(px(90.0))
                .h(px(22.0))
                .px(px(style::SPACE6))
                .rounded(px(5.0))
                .border_1()
                .border_color(style::separator())
                .value(move || settings_core::port_value(&value_m.core_settings(), key).to_owned())
                .on_change(move |v: &str| edit_m.edit_port(key, v.to_owned()))
                .placeholder(m.tr("ui.placeholder.port"))
                .text_size(px(font::BODY))
                .text_right()
                .text_color(style::primary_label()),
        )
}

fn action_button(label: impl Fn() -> String + 'static, icon: impl Fn() -> String + 'static, tint: impl Fn() -> AdaptiveColor + 'static, on_press: impl Fn() + 'static) -> impl IntoElement {
    view()
        .flex_1()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(style::SPACE4))
        .h(px(26.0))
        .rounded(px(style::CORNER_RADIUS))
        .border_1()
        .border_color(style::separator())
        .bg(style::fill(0.03))
        .hover(|s| s.bg(style::fill(0.08)))
        .on_press(on_press)
        .child(image(move || ImageSrc::from(icon())).w(px(12.0)).h(px(12.0)).tint_color(move || tint()))
        .child(text(label).truncate().text_size(px(font::CALLOUT)).text_color(style::primary_label()))
}

fn card() -> View {
    view().rounded(px(style::CORNER_RADIUS)).bg(style::fill(0.035)).pb(px(style::SPACE4))
}

pub fn settings_tab(m: Model) -> impl IntoElement {
    // Basic settings.
    let basic = {
        let (l1, l2, a1, a2, a3) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
        card()
            .child(card_header(icons::sliders(), {
                let m = m.clone();
                move || m.tr("ui.section.basic_settings")
            }))
            .child(toggle_row(
                icons::person_check(),
                {
                    let m = m.clone();
                    move || m.tr("ui.settings.launch_at_login")
                },
                move || l1.launch_at_login.get(),
                || false,
                move |on| {
                    let m = l2.clone();
                    run_root(async move { m.apply_launch_at_login(on).await });
                },
            ))
            .child(toggle_row(
                icons::power_circle(),
                {
                    let m = m.clone();
                    move || m.tr("ui.settings.auto_start_core")
                },
                move || a1.settings.with(|s| s.auto_start_core),
                || false,
                move |on| a2.settings.update(|s| s.auto_start_core = on),
            ))
            .child(selection_row(
                &m,
                icons::menubar(),
                "ui.settings.menu_bar_style",
                &[StatusBarDisplayMode::IconAndSpeed, StatusBarDisplayMode::IconOnly, StatusBarDisplayMode::SpeedOnly],
                {
                    let m = m.clone();
                    move || m.settings.with(|s| s.status_bar_display_mode)
                },
                |m, mode| {
                    m.tr(match mode {
                        StatusBarDisplayMode::IconAndSpeed => "ui.settings.display_mode.icon_and_speed",
                        StatusBarDisplayMode::IconOnly => "ui.settings.display_mode.icon_only",
                        StatusBarDisplayMode::SpeedOnly => "ui.settings.display_mode.speed_only",
                    })
                },
                {
                    let m = m.clone();
                    move |mode| m.settings.update(|s| s.status_bar_display_mode = mode)
                },
            ))
            .child(selection_row(
                &m,
                icons::book(),
                "ui.settings.language",
                &[AppLanguage::ZhHans, AppLanguage::En],
                {
                    let m = m.clone();
                    move || m.settings.with(|s| s.ui_language)
                },
                |m, l| m.tr(if l == AppLanguage::ZhHans { "ui.language.zh_hans" } else { "ui.language.en" }),
                {
                    let m = m.clone();
                    move |l| m.settings.update(|s| s.ui_language = l)
                },
            ))
            .child(selection_row(
                &m,
                icons::half_circle(),
                "ui.settings.appearance",
                &[AppearanceMode::System, AppearanceMode::Light, AppearanceMode::Dark],
                move || a3.settings.with(|s| s.appearance_mode),
                |m, a| {
                    m.tr(match a {
                        AppearanceMode::System => "ui.settings.appearance.system",
                        AppearanceMode::Light => "ui.settings.appearance.light",
                        AppearanceMode::Dark => "ui.settings.appearance.dark",
                    })
                },
                {
                    let m = m.clone();
                    move |a| m.settings.update(|s| s.appearance_mode = a)
                },
            ))
            .child(selection_row(
                &m,
                icons::text_alignleft(),
                "ui.settings.log_level",
                &crate::data::LOG_LEVELS,
                {
                    let m = m.clone();
                    move || {
                        let level = m.core_settings().log_level;
                        crate::data::LOG_LEVELS.iter().copied().find(|l| *l == level).unwrap_or("info")
                    }
                },
                |_, l| l.to_owned(),
                {
                    let m = m.clone();
                    move |l: &'static str| {
                        let m = m.clone();
                        run_root(async move { m.set_core_log_level(l).await });
                    }
                },
            ))
    };

    // Core settings.
    let core = {
        let syncing = {
            let m = m.clone();
            move || m.settings_syncing.with(|s| s.is_some())
        };
        let toggle = |icon: String, key: &'static str, title_key: &'static str, get: fn(&crate::settings::CoreSettings) -> bool| {
            let (t, g, s) = (m.clone(), m.clone(), m.clone());
            toggle_row(icon, move || t.tr(title_key), move || get(&g.core_settings()), syncing.clone(), move |on| {
                let m = s.clone();
                run_root(async move { m.set_core_bool(key, on).await });
            })
        };
        card()
            .child(card_header(icons::gears(), {
                let m = m.clone();
                move || m.tr("ui.section.core_settings")
            }))
            .child(toggle(icons::network(), "allow-lan", "ui.settings.allow_lan", |s| s.allow_lan))
            .child(toggle(icons::globe(), "ipv6", "ui.settings.ipv6", |s| s.ipv6))
            .child(toggle(icons::nodes_graph(), "tcp-concurrent", "ui.settings.tcp_concurrent", |s| s.tcp_concurrent))
    };

    // Proxy ports.
    let port_icons = [icons::network(), icons::wave(), icons::merge(), icons::turn_right(), icons::shield()];
    let ports = card()
        .child(card_header(icons::nodes_graph(), {
            let m = m.clone();
            move || m.tr("ui.section.proxy_ports")
        }))
        .child(
            view().gap(px(style::SPACE4)).px(px(style::SPACE4)).py(px(style::SPACE4)).children(
                settings_core::PORT_FIELDS
                    .iter()
                    .zip(port_icons)
                    .map(|(&(key, label), icon)| port_row(&m, key, label, icon))
                    .collect::<Vec<_>>(),
            ),
        );

    // Maintenance.
    let maintenance = {
        let (f1, f2, g1, g2, g3, d1) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
        card()
            .child(card_header(icons::wrench(), {
                let m = m.clone();
                move || m.tr("ui.section.maintenance")
            }))
            .child(
                view()
                    .gap(px(style::SPACE4))
                    .px(px(style::SPACE4))
                    .py(px(style::SPACE4))
                    .child(
                        view()
                            .flex_row()
                            .gap(px(style::SPACE6))
                            .child(action_button(
                                {
                                    let m = m.clone();
                                    move || m.tr("ui.action.flush_fakeip_cache")
                                },
                                icons::drive_minus,
                                style::secondary_label,
                                move || {
                                    let m = f1.clone();
                                    run_root(async move { m.flush_fakeip_cache().await });
                                },
                            ))
                            .child(action_button(
                                {
                                    let m = m.clone();
                                    move || m.tr("ui.action.flush_dns_cache")
                                },
                                icons::shield_network,
                                style::secondary_label,
                                move || {
                                    let m = f2.clone();
                                    run_root(async move { m.flush_dns_cache().await });
                                },
                            )),
                    )
                    .child(
                        view()
                            .flex_row()
                            .gap(px(style::SPACE6))
                            .child(action_button(
                                {
                                    let m = m.clone();
                                    move || m.tr("ui.action.update_geo_database")
                                },
                                move || match g1.geo_update.get() {
                                    TaskState::Running => icons::timer(),
                                    TaskState::Succeeded => icons::checkmark_circle_fill(),
                                    TaskState::Failed(_) => icons::warning_triangle(),
                                    _ => icons::arrow_down_circle_line(),
                                },
                                move || match g2.geo_update.get() {
                                    TaskState::Succeeded => style::positive(),
                                    TaskState::Failed(_) => style::negative(),
                                    _ => style::secondary_label(),
                                },
                                move || {
                                    if g3.is_live_untracked() {
                                        let m = g3.clone();
                                        run_root(async move { m.upgrade_geo().await });
                                    }
                                },
                            ))
                            .child(action_button(
                                {
                                    let m = m.clone();
                                    move || m.tr("ui.action.open_core_directory")
                                },
                                icons::folder,
                                style::secondary_label,
                                move || d1.show_core_dir(),
                            )),
                    ),
            )
    };

    // Feedback over the cards (ClashBar's `settingsFeedbackBanner`).
    let feedback_m = m.clone();
    let banner = when(
        {
            let m = m.clone();
            move || m.settings_feedback.with(|f| f.is_some())
        },
        move || {
            let m = feedback_m.clone();
            let is_error = {
                let m = m.clone();
                move || matches!(m.settings_feedback.get(), Some(Feedback::Error(_)))
            };
            let e2 = is_error.clone();
            let e3 = is_error.clone();
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .px(px(style::SPACE8))
                .py(px(style::SPACE6))
                .rounded(px(style::CORNER_RADIUS))
                .bg(move || if is_error() { style::negative().opacity(0.14) } else { style::positive().opacity(0.14) })
                .child(image(move || ImageSrc::from(if e2() { icons::warning_triangle() } else { icons::checkmark_circle_fill() })).w(px(12.0)).h(px(12.0)).tint_color(move || if e3() { style::negative() } else { style::positive() }))
                .child(
                    text(move || match m.settings_feedback.get() {
                        Some(Feedback::Saved(t)) | Some(Feedback::Error(t)) => t,
                        None => String::new(),
                    })
                    .flex_1()
                    .text_size(px(font::CALLOUT))
                    .text_color(style::primary_label()),
                )
        },
    );

    view()
        .gap(px(style::SPACE6))
        .child(banner)
        .child(basic.child(exceptions_section(m.clone())))
        .child(core)
        .child(ports)
        .child(maintenance)
}

/// The system proxy's bypass list (ClashBar's exceptions editor in the
/// basic settings card).
fn exceptions_section(m: Model) -> impl IntoElement {
    let expanded = signal(false);
    let (count_m, list_m, input_m, add_m, restore_m, save_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let header = view()
        .flex_row()
        .items_center()
        .px(px(style::SPACE4))
        .py(px(style::SPACE4))
        .on_press(move || expanded.set(!expanded.get()))
        .child(row_label(icons::turn_right(), {
            let m = m.clone();
            move || m.tr("ui.section.system_proxy_exceptions")
        }))
        .child(view().flex_1())
        .child(
            text(move || {
                let n = count_m.exceptions_draft.with(|d| d.len());
                if n > 0 { n.to_string() } else { String::new() }
            })
            .mr(px(style::SPACE2))
            .text_size(px(font::CAPTION))
            .text_color(style::secondary_label()),
        )
        .child(image(move || ImageSrc::from(if expanded.get() { icons::chevron_down() } else { icons::chevron_right() })).w(px(8.0)).h(px(8.0)).tint_color(style::tertiary_label()));
    let body = move || {
        let (list_m, input_m, add_m, restore_m, save_m) = (list_m.clone(), input_m.clone(), add_m.clone(), restore_m.clone(), save_m.clone());
        let hint_m = list_m.clone();
        let empty_m = list_m.clone();
        let add_label = add_m.clone();
        view()
            .gap(px(style::SPACE4))
            .px(px(style::SPACE4))
            .pb(px(style::SPACE4))
            .child(text(move || hint_m.tr("ui.settings.system_proxy_exceptions_hint")).text_size(px(font::CAPTION)).text_color(style::secondary_label()))
            .child(when(
                {
                    let m = list_m.clone();
                    move || m.exceptions_draft.with(|d| d.is_empty())
                },
                move || text({
                    let m = empty_m.clone();
                    move || m.tr("ui.settings.system_proxy_exceptions_empty")
                }).text_size(px(font::BODY)).text_color(style::secondary_label()),
            ))
            .child(view().gap(px(style::SPACE2)).child(each(
                {
                    let m = list_m.clone();
                    move || m.exceptions_draft.with(|d| d.iter().cloned().enumerate().collect::<Vec<_>>())
                },
                |(i, v): &(usize, String)| format!("{i}:{v}"),
                move |row, _| {
                    let m = list_m.clone();
                    view()
                        .flex_row()
                        .items_center()
                        .gap(px(style::SPACE4))
                        .px(px(style::SPACE6))
                        .h(px(22.0))
                        .rounded(px(5.0))
                        .bg(style::fill(0.05))
                        .child(text(move || row.with(|(_, v)| v.clone())).flex_1().truncate().text_size(px(font::BODY)).text_color(style::primary_label()))
                        .child(
                            view()
                                .tooltip(text({
                                    let m = m.clone();
                                    move || m.tr("ui.action.remove_exception")
                                }))
                                .on_press(move || m.remove_exception_draft(row.with_untracked(|(i, _)| *i)))
                                .child(icon_view(icons::xmark(), 9.0, style::tertiary_label)),
                        )
                },
            )))
            .child(
                view()
                    .flex_row()
                    .gap(px(style::SPACE4))
                    .child(
                        input()
                            .flex_1()
                            .h(px(22.0))
                            .px(px(style::SPACE6))
                            .rounded(px(5.0))
                            .border_1()
                            .border_color(style::separator())
                            .value(move || input_m.new_exception.get())
                            .on_change({
                                let m = input_m.clone();
                                move |v: &str| m.new_exception.set(v.to_owned())
                            })
                            .on_submit({
                                let m = input_m.clone();
                                move |_: &str| m.add_exception_draft()
                            })
                            .placeholder(input_m.tr("ui.placeholder.system_proxy_exception"))
                            .text_size(px(font::BODY))
                            .text_color(style::primary_label()),
                    )
                    .child(action_button(move || add_label.tr("ui.action.add_exception"), icons::checkmark, style::secondary_label, move || add_m.add_exception_draft())),
            )
            .child(
                view()
                    .flex_row()
                    .gap(px(style::SPACE4))
                    .child(action_button(
                        {
                            let m = restore_m.clone();
                            move || m.tr("ui.action.restore_defaults")
                        },
                        icons::arrow_clockwise,
                        style::secondary_label,
                        move || restore_m.restore_default_exceptions_draft(),
                    ))
                    .child(action_button(
                        {
                            let m = save_m.clone();
                            move || m.tr("ui.action.save_exceptions")
                        },
                        icons::checkmark_circle_fill,
                        style::positive,
                        move || {
                            let m = save_m.clone();
                            run_root(async move { m.save_exceptions().await });
                        },
                    )),
            )
    };
    view().child(header).child(when(move || expanded.get(), body))
}
