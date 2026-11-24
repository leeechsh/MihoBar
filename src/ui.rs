//! The panel (ClashBar's `MenuBarRootView`): header, mode switcher, tabs,
//! the current tab's content and the footer.

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::icons;
use crate::model::{CoreMode, Model, RootTab, RuntimeStatus, StartTrigger};
use crate::style::{self, font};

const PROJECT_HOMEPAGE: &str = "https://github.com/Sitoi/ClashBar";
const MIHOMO_REPOSITORY: &str = "https://github.com/MetaCubeX/mihomo";

/// An SVG icon tinted `color`, `size` points square.
fn icon(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

pub fn panel(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    view()
        .w(px(style::PANEL_WIDTH))
        .flex_1()
        .px(px(style::SPACE8))
        .child(header(m.clone(), win.clone()))
        .child(mode_and_tabs(m.clone()))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() != RootTab::Rules
            },
            {
                let m = m.clone();
                move || scroll().flex_1().child(tab_content(m.clone(), win.clone()))
            },
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() == RootTab::Rules
            },
            {
                let m = m.clone();
                move || crate::ui_tabs::rules_tab(m.clone())
            },
        ))
        .child(footer(m))
}

// --- Header ------------------------------------------------------------------

fn header(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    let logo = view()
        .w(px(34.0))
        .h(px(34.0))
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .on_press(|| {
            let _ = open_url(PROJECT_HOMEPAGE);
        })
        .child(icon(icons::brand_logo(), 26.0, style::primary_label));

    let title = view()
        .gap(px(style::SPACE2))
        .child(
            text("Mihobar")
                .text_size(px(font::TITLE3))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(style::primary_label()),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .child(connection_control(m.clone(), win)),
        );

    let actions = {
        let pin_m = m.clone();
        let restart_m = m.clone();
        let toggle_m = m.clone();
        let quit_m = m.clone();
        let pinned = {
            let m = m.clone();
            move || m.settings.with(|s| s.pinned)
        };
        view()
            .flex_row()
            .items_center()
            .gap(px(style::SPACE6))
            .child(top_icon(
                {
                    let pinned = pinned.clone();
                    move || if pinned() { icons::pin_fill() } else { icons::pin() }
                },
                {
                    let pinned = pinned.clone();
                    move || if pinned() { style::info() } else { style::tertiary_label() }
                },
                {
                    let m = m.clone();
                    move || m.tr(if m.settings.with(|s| s.pinned) { "ui.action.unpin" } else { "ui.action.pin" })
                },
                || false,
                move || pin_m.settings.update(|s| s.pinned = !s.pinned),
            ))
            .child(top_icon(
                icons::arrow_clockwise,
                style::info,
                {
                    let m = m.clone();
                    move || m.tr(if m.is_running() { "app.primary.restart" } else { "app.primary.start" })
                },
                {
                    let m = m.clone();
                    move || m.is_processing() || m.is_remote()
                },
                move || {
                    let m = restart_m.clone();
                    crate::model::run_root(async move { m.primary_core_action().await });
                },
            ))
            .child(top_icon(
                {
                    let m = m.clone();
                    move || if m.is_running() { icons::stop_circle() } else { icons::play_circle() }
                },
                {
                    let m = m.clone();
                    move || if m.is_running() { style::warning() } else { style::positive() }
                },
                {
                    let m = m.clone();
                    move || m.tr(if m.is_running() { "ui.action.stop" } else { "app.primary.start" })
                },
                {
                    let m = m.clone();
                    move || m.is_processing() || m.is_remote()
                },
                move || {
                    let m = toggle_m.clone();
                    crate::model::run_root(async move {
                        if m.status.get_untracked() == RuntimeStatus::Running {
                            m.stop_core().await;
                        } else {
                            m.start_core(StartTrigger::Manual).await;
                        }
                    });
                },
            ))
            .child(top_icon(
                icons::power,
                style::negative,
                {
                    let m = m.clone();
                    move || m.tr("ui.action.quit")
                },
                || false,
                move || {
                    let m = quit_m.clone();
                    crate::model::run_root(async move { m.quit().await });
                },
            ))
    };

    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE8))
        .py(px(style::SPACE8))
        .child(view().flex_row().items_start().gap(px(style::SPACE8)).child(logo).child(title))
        .child(view().flex_1())
        .child(actions)
}

/// A 26-point icon button (ClashBar's `compactTopIcon`).
fn top_icon(
    src: impl Fn() -> String + 'static,
    tone: impl Fn() -> AdaptiveColor + 'static,
    label: impl Fn() -> String + 'static,
    disabled: impl Fn() -> bool + Clone + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let label = std::rc::Rc::new(label);
    let is_disabled = disabled.clone();
    view()
        .w(px(26.0))
        .h(px(26.0))
        .items_center()
        .justify_center()
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.08)))
        .opacity(move || if disabled() { 0.6_f32 } else { 1.0_f32 })
        .tooltip(text({
            let label = label.clone();
            move || label()
        }))
        .label(move || label())
        .on_press(move || {
            if !is_disabled() {
                on_press();
            }
        })
        .child(image(move || ImageSrc::from(src())).w(px(16.0)).h(px(16.0)).tint_color(move || tone()))
}

/// The machine the panel talks to, its controller address and state; a
/// press opens the machine menu (ClashBar's `headerConnectionControl`).
fn connection_control(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    let dot_m = m.clone();
    let spin_m = m.clone();
    let press_m = m.clone();
    let row = element_ref();
    view()
        .element_ref(&row)
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .px(px(style::SPACE6))
        .h(px(18.0))
        .rounded(px(style::CORNER_RADIUS))
        .bg(style::fill(0.06))
        .hover(|s| s.bg(style::fill(0.10)))
        .opacity(move || if spin_m.switching_machine.get() { 0.6_f32 } else { 1.0_f32 })
        .label({
            let m = m.clone();
            move || m.tr("ui.machine.manage")
        })
        .on_press(move || {
            let m = press_m.clone();
            let (manage_m, win) = (m.clone(), win.clone());
            crate::model::run_root(async move {
                m.show_machine_menu(move || {
                    crate::ui_proxy::open_side(&manage_m, &win, row.clone(), crate::model::SideKind::Machines);
                })
                .await;
            });
        })
        .child(
            view().w(px(8.0)).h(px(8.0)).rounded(px(4.0)).bg(move || {
                if dot_m.switching_machine.get() {
                    return style::warning();
                }
                if let Some(machine) = dot_m.remote.get() {
                    return match dot_m.machine_status(&machine.id) {
                        crate::remote::MachineStatus::Connected(_) => style::positive(),
                        crate::remote::MachineStatus::Failed(_) => style::negative(),
                        crate::remote::MachineStatus::Checking => style::warning(),
                        crate::remote::MachineStatus::Unknown => style::tertiary_label(),
                    };
                }
                match dot_m.status.get() {
                    RuntimeStatus::Running => style::positive(),
                    RuntimeStatus::Starting => style::warning(),
                    RuntimeStatus::Failed => style::negative(),
                    RuntimeStatus::Stopped => style::tertiary_label(),
                }
            }),
        )
        .child(
            text({
                let m = m.clone();
                move || match m.remote.get() {
                    Some(machine) => format!("{} · {}", machine.name, machine.display_address()),
                    None => format!("{} · {}", m.tr("ui.machine.local"), m.target.with(|t| t.controller.clone())),
                }
            })
            .max_w(px(170.0))
            .truncate()
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(style::primary_label()),
        )
        .child(icon(icons::chevron_up_down(), 8.0, style::tertiary_label))
}

// --- Mode switcher and tabs --------------------------------------------------

fn mode_and_tabs(m: Model) -> impl IntoElement {
    let modes = CoreMode::ALL.map(|mode| mode_button(m.clone(), mode));
    let tabs = RootTab::ALL.map(|tab| tab_button(m.clone(), tab));
    view()
        .gap(px(style::SPACE2))
        .child(view().flex_row().gap(px(style::SPACE2)).children(modes))
        .child(view().flex_row().gap(px(style::SPACE2)).children(tabs))
}

fn mode_icon(mode: CoreMode) -> String {
    match mode {
        CoreMode::Rule => icons::shield(),
        CoreMode::Global => icons::globe(),
        CoreMode::Direct => icons::arrow_direct(),
    }
}

fn mode_button(m: Model, mode: CoreMode) -> impl IntoElement {
    let selected = {
        let m = m.clone();
        move || m.mode.get() == Some(mode)
    };
    let enabled = {
        let m = m.clone();
        move || m.is_mode_switch_enabled()
    };
    let press_m = m.clone();
    let press_enabled = enabled.clone();
    view()
        .flex_1()
        .h(px(28.0))
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(style::SPACE4))
        .rounded(px(style::CORNER_RADIUS))
        .border_1()
        .border_color({
            let selected = selected.clone();
            move || if selected() { style::accent(0.35) } else { style::accent(0.0) }
        })
        .bg({
            let selected = selected.clone();
            move || if selected() { style::accent(0.16) } else { style::fill(0.0) }
        })
        .hover(|s| s.bg(style::fill(0.06)))
        .opacity({
            let enabled = enabled.clone();
            move || if enabled() { 1.0_f32 } else { 0.55_f32 }
        })
        .on_press(move || {
            if press_enabled() {
                let m = press_m.clone();
                crate::model::run_root(async move { m.switch_mode(mode).await });
            }
        })
        .child(image(ImageSrc::from(mode_icon(mode))).w(px(13.0)).h(px(13.0)).tint_color({
            let selected = selected.clone();
            move || if selected() { style::info() } else { style::secondary_label() }
        }))
        .child(
            text({
                let m = m.clone();
                move || m.tr(mode.title_key())
            })
            .text_size(px(font::CALLOUT))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(move || if selected() { style::info() } else { style::primary_label() }),
        )
}

fn tab_button(m: Model, tab: RootTab) -> impl IntoElement {
    let selected = {
        let m = m.clone();
        move || m.tab.get() == tab
    };
    let press_m = m.clone();
    view()
        .flex_1()
        .h(px(28.0))
        .items_center()
        .justify_center()
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.06)))
        .on_press(move || press_m.tab.set(tab))
        .child(
            text({
                let m = m.clone();
                move || m.tr(tab.title_key())
            })
            .text_size(px(font::CALLOUT))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color({
                let selected = selected.clone();
                move || if selected() { style::primary_label() } else { style::secondary_label() }
            }),
        )
        .child(
            view()
                .absolute()
                .bottom(px(0.0))
                .w(px(18.0))
                .h(px(2.0))
                .rounded(px(1.0))
                .bg(move || if selected() { style::info() } else { style::accent(0.0) }),
        )
}

// --- Tab content -------------------------------------------------------------

fn tab_content(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    let (proxy_m, conn_m, logs_m) = (m.clone(), m.clone(), m.clone());
    view()
        .py(px(style::SPACE6))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() == RootTab::Proxy
            },
            move || crate::ui_proxy::proxy_tab(proxy_m.clone(), win.clone()),
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() == RootTab::Connections
            },
            move || crate::ui_tabs::connections_tab(conn_m.clone()),
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() == RootTab::Logs
            },
            move || crate::ui_tabs::logs_tab(logs_m.clone()),
        ))
        .child(when(
            {
                let m = m.clone();
                move || m.tab.get() == RootTab::System
            },
            move || crate::ui_settings::settings_tab(m.clone()),
        ))
}

// --- Footer ------------------------------------------------------------------

fn footer(m: Model) -> impl IntoElement {
    let version_m = m.clone();
    view()
        .flex_row()
        .items_center()
        .h(px(28.0))
        .border_t_1()
        .border_color(style::separator())
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE4))
                .on_press(|| {
                    let _ = open_url(MIHOMO_REPOSITORY);
                })
                .child(image(ImageSrc::from(icons::cpu())).w(px(11.0)).h(px(11.0)).tint_color(style::secondary_label()))
                .child(
                    text(move || {
                        let v = version_m.version.get();
                        let v = v.trim();
                        let shown = if v.is_empty() {
                            version_m.tr("ui.common.unknown")
                        } else if v.starts_with(|c: char| c.is_ascii_digit()) {
                            format!("v{v}")
                        } else {
                            v.to_owned()
                        };
                        version_m.tr_args("ui.footer.core_mihomo", &[&shown])
                    })
                    .text_size(px(font::CAPTION))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(style::secondary_label()),
                ),
        )
        .child(core_upgrade_button(m.clone()))
        .child(view().flex_1())
        .child(
            text(move || m.tr_args("ui.footer.version", &[&env!("CARGO_PKG_VERSION")]))
                .text_size(px(font::CAPTION))
                .text_color(style::tertiary_label()),
        )
}

/// The footer's core upgrade (ClashBar's footer `core_upgrade` control).
fn core_upgrade_button(m: Model) -> impl IntoElement {
    use crate::settings_core::TaskState;
    let state = {
        let m = m.clone();
        move || m.core_upgrade.get()
    };
    let (s1, s2, s3) = (state.clone(), state.clone(), state.clone());
    let (help_m, label_m, press_m) = (m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE2))
        .ml(px(style::SPACE6))
        .px(px(style::SPACE4))
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.08)))
        .opacity({
            let m = m.clone();
            move || if m.is_live() { 1.0_f32 } else { 0.5_f32 }
        })
        .tooltip(text(move || {
            let m = &help_m;
            if !m.is_live() {
                return m.tr("ui.footer.core_upgrade.help.disabled");
            }
            match s1() {
                TaskState::Running => m.tr("ui.footer.core_upgrade.help.running"),
                TaskState::Succeeded => m.tr("ui.footer.core_upgrade.help.success"),
                TaskState::AlreadyLatest(Some(v)) => m.tr_args("ui.footer.core_upgrade.help.latest_version", &[&v]),
                TaskState::AlreadyLatest(None) => m.tr("ui.footer.core_upgrade.help.latest"),
                TaskState::Failed(e) => m.tr_args("ui.footer.core_upgrade.help.failed", &[&e]),
                TaskState::Idle => m.tr("ui.footer.core_upgrade.help"),
            }
        }))
        .label({
            let m = m.clone();
            move || m.tr("ui.action.upgrade_core")
        })
        .on_press(move || {
            if press_m.is_live_untracked() {
                let m = press_m.clone();
                crate::model::run_root(async move { m.upgrade_core().await });
            }
        })
        .child(
            image(move || ImageSrc::from(match s2() {
                TaskState::Running => icons::timer(),
                TaskState::Succeeded | TaskState::AlreadyLatest(_) => icons::checkmark_circle_fill(),
                TaskState::Failed(_) => icons::warning_triangle(),
                TaskState::Idle => icons::arrow_down_circle_line(),
            }))
            .w(px(10.0))
            .h(px(10.0))
            .tint_color(move || match s3() {
                TaskState::Succeeded | TaskState::AlreadyLatest(_) => style::positive(),
                TaskState::Failed(_) => style::negative(),
                _ => style::secondary_label(),
            }),
        )
        .child(
            text(move || match state() {
                TaskState::Running => label_m.tr("ui.footer.core_upgrade.running"),
                TaskState::Succeeded => label_m.tr("ui.footer.core_upgrade.success"),
                TaskState::AlreadyLatest(_) => label_m.tr("ui.footer.core_upgrade.latest"),
                TaskState::Failed(_) => label_m.tr("ui.footer.core_upgrade.failed"),
                TaskState::Idle => String::new(),
            })
            .text_size(px(font::CAPTION))
            .text_color(style::secondary_label()),
        )
}
