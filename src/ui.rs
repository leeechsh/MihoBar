//! The panel (ClashBar's `MenuBarRootView`): header, mode switcher, tabs,
//! the current tab's content and the footer.

use nsh_core::element::ImageSrc;
use nsh_core::prelude::*;

use crate::icons;
use crate::model::{Model, RootTab, RuntimeStatus, StartTrigger};
use crate::style::{self, font};

const PROJECT_HOMEPAGE: &str = "https://github.com/Sitoi/ClashBar";
const MIHOMO_REPOSITORY: &str = "https://github.com/MetaCubeX/mihomo";

/// An SVG icon tinted `color`, `size` points square.
fn icon(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

pub fn panel(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    let column = element_ref();
    fit_window_to_content(m.clone(), win.clone(), column.clone());
    // The window is as tall as the panel's content; the whole panel
    // scrolls only when that is taller than the screen, with no scroll bar.
    scroll().flex_1().scrollbar(false).child(
        view()
            .element_ref(&column)
            .w(px(style::PANEL_WIDTH))
            .px(px(style::SPACE8))
            .child(header(m.clone()))
            .child(tabs(m.clone()))
            .child(crate::widgets::entrance_on(
                {
                    let m = m.clone();
                    move || m.tab.get()
                },
                tab_content(m.clone(), win.clone()),
            ))
            .child(footer(m)),
    )
}

/// Keeps the window as tall as the panel's content, up to the bottom of
/// the screen, resizing with macOS's own animation: at once when the tab
/// changes, and as content comes and goes while the panel shows.
fn fit_window_to_content(m: Model, win: nsh_core::element::desktop::WindowRef, column: ElementRef) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed) + 1;
    // A new panel window (one each time it opens) starts at its default
    // size, and the height set for the last one was its own: it opens at
    // the height the tab it opens on had last (the node tab's, not the
    // taller settings' it may have closed on), and its first fit lands at
    // once, as it appears, rather than easing.
    LAST_HEIGHT.with(|h| h.set(None));
    if let Some(h) = tab_height(m.tab.get_untracked()) {
        win.set_bounds(nsh_core::desktop::WindowFrame {
            width: Some(Points(style::PANEL_WIDTH)),
            height: Some(Points(h)),
            ..Default::default()
        });
        LAST_HEIGHT.with(|last| last.set(Some(h)));
    }
    let first = std::rc::Rc::new(std::cell::Cell::new(true));
    {
        let (m, win, column, first) = (m.clone(), win.clone(), column.clone(), first.clone());
        effect(move || {
            let tab = m.tab.get();
            let (win, column, first) = (win.clone(), column.clone(), first.clone());
            untracked(|| {
                crate::model::run_root(async move {
                    fit_trace("tab changed");
                    // The new tab's views lay out first (two frames).
                    timer(std::time::Duration::from_millis(32)).await;
                    fit_once(&win, &column, tab, &first).await;
                });
            });
        });
    }
    crate::model::run_root(async move {
        while m.panel_visible.get_untracked() && GENERATION.load(Ordering::Relaxed) == generation {
            if !RESIZING.load(Ordering::Relaxed) {
                fit_once(&win, &column, m.tab.get_untracked(), &first).await;
            }
            timer(std::time::Duration::from_millis(250)).await;
        }
    });
}

/// Resizes the window to the panel's content: eased over about 180 ms
/// in frame steps (macOS's own animated resize re-enters NSH's animation
/// pass), at once when the user asks for less motion.
/// Debug trace of the window fitting (MIHOBAR_FIT_TRACE=<file>).
pub(crate) fn fit_trace(line: &str) {
    if let Some(path) = std::env::var_os("MIHOBAR_FIT_TRACE") {
        use std::io::Write;
        let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() % 100_000);
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{ms:>6} {line}");
        }
    }
}

thread_local! {
    /// The window height this code last set: the state NSH reports lags
    /// behind `set_bounds`, and easing from that stale height made the
    /// panel's bottom jump back and ease again.
    static LAST_HEIGHT: std::cell::Cell<Option<f32>> = const { std::cell::Cell::new(None) };
}

thread_local! {
    /// Each tab's height when the panel last fitted it (node, system).
    static TAB_HEIGHTS: std::cell::Cell<[Option<f32>; 2]> = const { std::cell::Cell::new([None, None]) };
}

fn tab_index(tab: crate::model::RootTab) -> usize {
    match tab {
        crate::model::RootTab::Proxy => 0,
        crate::model::RootTab::System => 1,
    }
}

fn tab_height(tab: crate::model::RootTab) -> Option<f32> {
    TAB_HEIGHTS.with(|t| t.get()[tab_index(tab)])
}

fn remember_tab_height(tab: crate::model::RootTab, h: f32) {
    TAB_HEIGHTS.with(|t| {
        let mut all = t.get();
        all[tab_index(tab)] = Some(h);
        t.set(all);
    });
}

/// A resize is easing (the periodic check leaves it alone).
static RESIZING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Fits the window to `tab`'s content; the first fit of a window that just
/// opened (`first`, cleared once a fit resizes) lands without easing.
async fn fit_once(win: &nsh_core::element::desktop::WindowRef, column: &ElementRef, tab: crate::model::RootTab, first: &std::cell::Cell<bool>) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static TWEEN: AtomicU64 = AtomicU64::new(0);
    let (Some(measured), Some(state)) = (column.measure().await, win.state()) else { return };
    let content = measured.frame.size.height.0.ceil();
    fit_trace(&format!("measure content={content} window={} last={:?}", state.bounds.size.height.0, LAST_HEIGHT.with(|h| h.get())));
    let bounds = state.bounds;
    let top = bounds.origin.y.0;
    let visible = crate::geometry::visible_frame_at(bounds.origin.x.0 + bounds.size.width.0 / 2.0);
    let max = (visible.max_y() - top - 8.0).max(240.0);
    let target = content.min(max);
    if content > 0.0 {
        remember_tab_height(tab, target.round());
    }
    let from = LAST_HEIGHT.with(|h| h.get()).unwrap_or(bounds.size.height.0);
    let width_off = LAST_HEIGHT.with(|h| h.get()).is_none() && (bounds.size.width.0 - style::PANEL_WIDTH).abs() > 0.5;
    if content <= 0.0 || ((target - from).abs() <= 0.5 && !width_off) {
        if content > 0.0 {
            first.set(false);
        }
        return;
    }
    let instant = first.replace(false);
    let set = |h: f32| {
        LAST_HEIGHT.with(|last| last.set(Some(h.round())));
        win.set_bounds(nsh_core::desktop::WindowFrame {
            width: Some(Points(style::PANEL_WIDTH)),
            height: Some(Points(h.round())),
            ..Default::default()
        });
    };
    let tween = TWEEN.fetch_add(1, Ordering::Relaxed) + 1;
    if instant || nsh_core::prelude::reduce_motion() || (target - from).abs() < 4.0 {
        set(target);
        return;
    }
    const STEPS: u32 = 11;
    RESIZING.store(true, Ordering::Relaxed);
    for step in 1..=STEPS {
        if TWEEN.load(Ordering::Relaxed) != tween {
            return; // A newer resize took over (and clears the flag).
        }
        let t = step as f32 / STEPS as f32;
        let eased = 1.0 - (1.0 - t).powi(3); // ease-out cubic
        set(from + (target - from) * eased);
        fit_trace(&format!("  set {}", (from + (target - from) * eased).round()));
        if step < STEPS {
            timer(std::time::Duration::from_millis(16)).await;
        }
    }
    RESIZING.store(false, Ordering::Relaxed);
}

// --- Header ------------------------------------------------------------------

fn header(m: Model) -> impl IntoElement {
    // The app icon: a rounded square in the macOS icon shape.
    let logo = view()
        .w(px(34.0))
        .h(px(34.0))
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .bg_linear_to_b()
        .gradient_from(style::indigo())
        .gradient_to(style::info())
        .shadow_sm()
        .on_press(|| {
            let _ = open_url(PROJECT_HOMEPAGE);
        })
        .child(icon(icons::brand_logo(), 20.0, style::white));

    let title = text("MihoBar")
        .text_size(px(font::TITLE3))
        .font_weight(FontWeight::BOLD)
        .text_color(style::primary_label());

    // macOS toolbar buttons: monochrome icons that take color only for a
    // state (the panel pinned, the core stopped) or, quitting, on hover;
    // evenly spaced.
    let actions = {
        let pin_m = m.clone();
        let restart_m = m.clone();
        let toggle_m = m.clone();
        let quit_m = m.clone();
        let pinned = {
            let m = m.clone();
            move || m.settings.with(|s| s.pinned)
        };
        let pin = top_icon(
            {
                let pinned = pinned.clone();
                move || if pinned() { icons::pin_fill() } else { icons::pin() }
            },
            {
                let pinned = pinned.clone();
                move |_| if pinned() { style::info() } else { style::secondary_label() }
            },
            {
                let m = m.clone();
                move || m.tr(if m.settings.with(|s| s.pinned) { "ui.action.unpin" } else { "ui.action.pin" })
            },
            || false,
            move || pin_m.settings.update(|s| s.pinned = !s.pinned),
        );
        let restart = top_icon(
            icons::arrow_clockwise,
            |_| style::secondary_label(),
            {
                let m = m.clone();
                move || m.tr(if m.is_running() { "app.primary.restart" } else { "app.primary.start" })
            },
            {
                let m = m.clone();
                move || m.is_processing()
            },
            move || {
                let m = restart_m.clone();
                crate::model::run_root(async move { m.primary_core_action().await });
            },
        );
        let toggle = top_icon(
            {
                let m = m.clone();
                move || if m.is_running() { icons::stop_circle() } else { icons::play_circle() }
            },
            {
                let m = m.clone();
                move |_| if m.is_running() { style::secondary_label() } else { style::positive() }
            },
            {
                let m = m.clone();
                move || m.tr(if m.is_running() { "ui.action.stop" } else { "app.primary.start" })
            },
            {
                let m = m.clone();
                move || m.is_processing()
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
        );
        let quit = top_icon(
            icons::power,
            |hovered| if hovered { style::negative() } else { style::secondary_label() },
            {
                let m = m.clone();
                move || m.tr("ui.action.quit")
            },
            || false,
            move || {
                let m = quit_m.clone();
                crate::model::run_root(async move { m.quit().await });
            },
        );
        // Evenly spaced, as a toolbar's buttons.
        view().flex_row().items_center().gap(px(style::SPACE4)).child(pin).child(restart).child(toggle).child(quit)
    };

    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE8))
        .pt(px(10.0))
        .pb(px(style::SPACE8))
        .px(px(style::SPACE2))
        .child(view().flex_row().items_center().gap(px(10.0)).child(logo).child(title))
        .child(view().flex_1())
        .child(actions)
}

/// A toolbar button: a borderless icon whose rounded-square background
/// shows on hover; `tone` gets whether the pointer is over it.
fn top_icon(
    src: impl Fn() -> String + 'static,
    tone: impl Fn(bool) -> AdaptiveColor + 'static,
    label: impl Fn() -> String + 'static,
    disabled: impl Fn() -> bool + Clone + 'static,
    on_press: impl Fn() + 'static,
) -> impl IntoElement {
    let label = std::rc::Rc::new(label);
    let is_disabled = disabled.clone();
    let hovered = signal(false);
    view()
        .w(px(26.0))
        .h(px(26.0))
        .items_center()
        .justify_center()
        .rounded(px(style::CONTROL_RADIUS + 1.0))
        .hover(|s| s.bg(style::fill(0.08)))
        .transition_colors()
        .duration(120.0)
        .opacity(move || if disabled() { 0.4_f32 } else { 1.0_f32 })
        .on_mouse_enter(move |_: &MouseEvent| hovered.set(true))
        .on_mouse_leave(move |_: &MouseEvent| hovered.set(false))
        .tooltip(crate::widgets::tip({
            let label = label.clone();
            move || label()
        }))
        .label(move || label())
        .on_press(move || {
            if !is_disabled() {
                on_press();
            }
        })
        .child(image(move || ImageSrc::from(src())).w(px(15.0)).h(px(15.0)).tint_color(move || tone(hovered.get())))
}

// --- Mode switcher and tabs --------------------------------------------------

/// The tabs: a toolbar of icons over labels (System Settings' toolbar).
/// (The outbound mode is a row of the proxy tab, with a native submenu.)
fn tabs(m: Model) -> impl IntoElement {
    let tabs = RootTab::ALL.map(|tab| tab_button(m.clone(), tab));
    // Rules, connections and logs open in the advanced window: a segment
    // that opens it rather than staying chosen.
    let manage_m = m.clone();
    let manage = tab_face(
        icons::desktop(),
        {
            let m = m.clone();
            move || m.tr("ui.manager.open")
        },
        || false,
    )
    .on_press(move || manage_m.open_manager(None));
    // A segmented control (AppKit's `NSSegmentedControl`): the segments on
    // a rounded track, the chosen one raised.
    view().pb(px(style::SPACE4)).child(
        view()
            .flex_row()
            .gap(px(2.0))
            .p(px(3.0))
            .rounded(px(style::TAB_RADIUS))
            .bg(style::control_track())
            .children(tabs)
            .child(manage),
    )
}

fn tab_icon(tab: RootTab) -> String {
    match tab {
        RootTab::Proxy => icons::globe(),
        RootTab::System => icons::gear(),
    }
}

fn tab_button(m: Model, tab: RootTab) -> impl IntoElement {
    let press_m = m.clone();
    tab_face(
        tab_icon(tab),
        {
            let m = m.clone();
            move || m.tr(tab.title_key())
        },
        move || m.tab.get() == tab,
    )
    .on_press(move || press_m.tab.set(tab))
}

/// A segment: its icon beside its label, raised and tinted while
/// `selected`.
fn tab_face(
    icon: String,
    label: impl Fn() -> String + 'static,
    selected: impl Fn() -> bool + Clone + 'static,
) -> nsh_core::element::View {
    let (s1, s2, s3) = (selected.clone(), selected.clone(), selected.clone());
    view()
        .flex_1()
        .flex_row()
        .h(px(26.0))
        .items_center()
        .justify_center()
        .gap(px(5.0))
        .rounded(px(style::TAB_RADIUS - 3.0))
        .bg(move || if s1() { style::elevated() } else { style::fill(0.0) })
        .border_1()
        .border_color(move || if s3() { style::card_border() } else { style::fill(0.0) })
        .transition_colors()
        .duration(140.0)
        .child(image(ImageSrc::from(icon)).w(px(13.0)).h(px(13.0)).tint_color(move || {
            if s2() { style::info() } else { style::secondary_label() }
        }))
        .child(
            text(label)
                .text_size(px(font::CALLOUT))
                .font_weight(FontWeight::MEDIUM)
                .text_color(move || if selected() { style::info() } else { style::secondary_label() }),
        )
}

// --- Tab content -------------------------------------------------------------

fn tab_content(m: Model, win: nsh_core::element::desktop::WindowRef) -> impl IntoElement {
    let proxy_m = m.clone();
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
        .h(px(30.0))
        .px(px(style::SPACE2))
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
        .child(app_version_button(m))
}

/// MihoBar's version, which checks for a newer one when pressed and shows
/// the outcome in its place; a newer version shows in blue and opens NSH's
/// update window.
fn app_version_button(m: Model) -> impl IntoElement {
    use crate::updates::CheckState;
    let (text_m, color_m, tip_m, label_m, press_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .px(px(style::SPACE4))
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.08)))
        .tooltip(crate::widgets::tip(move || {
            match tip_m.app_check.get() {
                CheckState::Available(v) => tip_m.tr_args("ui.footer.app_update.install", &[&v]),
                _ => tip_m.tr("ui.footer.app_update.check"),
            }
        }))
        .label(move || label_m.tr("ui.footer.app_update.check"))
        .on_press(move || {
            let m = press_m.clone();
            if matches!(m.app_check.get_untracked(), CheckState::Available(_)) {
                m.install_app_update();
            } else if m.app_check.get_untracked() != CheckState::Checking {
                crate::model::run_root(async move { m.check_app_version(true).await });
            }
        })
        .child(
            text(move || {
                let m = &text_m;
                let version = m.tr_args("ui.footer.version", &[&crate::updates::app_version()]);
                match m.app_check.get() {
                    CheckState::Checking => m.tr("ui.updates.checking"),
                    CheckState::UpToDate => format!("{version} · {}", m.tr("ui.updates.up_to_date")),
                    CheckState::Failed => m.tr("ui.updates.failed"),
                    CheckState::Available(v) => m.tr_args("ui.footer.app_update.available", &[&v]),
                    _ => version,
                }
            })
            .text_size(px(font::CAPTION))
            .font_weight(FontWeight::MEDIUM)
            .text_color(move || match color_m.app_check.get() {
                CheckState::Available(_) => style::info(),
                CheckState::Failed => style::negative(),
                _ => style::tertiary_label(),
            }),
        )
}

/// The footer's core control: checks for a newer core, and once one is
/// found upgrades to it (mihomo's `/upgrade`, the core running); the check
/// or the upgrade shows beside it.
fn core_upgrade_button(m: Model) -> impl IntoElement {
    use crate::settings_core::TaskState;
    use crate::updates::CheckState;
    /// What the control shows: the upgrade's state over the check's.
    #[derive(Clone, PartialEq)]
    enum Shown {
        Idle,
        Checking,
        UpToDate,
        Latest(String),
        Available(String),
        Upgrading,
        Upgraded,
        Failed,
    }
    let shown = {
        let m = m.clone();
        move || match m.core_upgrade.get() {
            TaskState::Running => Shown::Upgrading,
            TaskState::Succeeded => Shown::Upgraded,
            TaskState::AlreadyLatest(_) => Shown::UpToDate,
            TaskState::Failed(_) => Shown::Failed,
            TaskState::Idle => match m.core_check.get() {
                CheckState::Checking => Shown::Checking,
                CheckState::UpToDate => Shown::UpToDate,
                CheckState::Failed => Shown::Failed,
                CheckState::Latest(v) => Shown::Latest(v),
                _ if m.core_update_available() => Shown::Available(m.core_latest.get().unwrap_or_default()),
                _ => Shown::Idle,
            },
        }
    };
    let (s1, s2, s3, s4, s5) = (shown.clone(), shown.clone(), shown.clone(), shown.clone(), shown.clone());
    let (tip_m, label_m, text_m, press_m) = (m.clone(), m.clone(), m.clone(), m.clone());
    let version = |v: &str| format!("v{}", v.trim_start_matches('v'));
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE2))
        .ml(px(style::SPACE6))
        .px(px(style::SPACE4))
        .rounded(px(style::CORNER_RADIUS))
        .hover(|s| s.bg(style::fill(0.08)))
        .tooltip(crate::widgets::tip(move || match s1() {
            Shown::Available(v) if tip_m.is_live() => tip_m.tr_args("ui.footer.core_update.install", &[&version(&v)]),
            Shown::Available(_) => tip_m.tr("ui.footer.core_upgrade.help.disabled"),
            _ => tip_m.tr("ui.footer.core_update.check"),
        }))
        .label(move || label_m.tr("ui.footer.core_update.check"))
        .on_press(move || {
            let m = press_m.clone();
            match s2() {
                Shown::Checking | Shown::Upgrading => {}
                Shown::Available(_) => {
                    if m.is_live_untracked() {
                        crate::model::run_root(async move { m.upgrade_core().await });
                    }
                }
                _ => crate::model::run_root(async move { m.check_core_update(false).await }),
            }
        })
        .child(
            image(move || {
                ImageSrc::from(match s3() {
                    Shown::Checking | Shown::Upgrading => icons::timer(),
                    Shown::UpToDate | Shown::Upgraded => icons::checkmark_circle_fill(),
                    Shown::Failed => icons::warning_triangle(),
                    Shown::Available(_) => icons::arrow_down_circle_line(),
                    _ => icons::arrows_circlepath(),
                })
            })
            .w(px(10.0))
            .h(px(10.0))
            .tint_color(move || match s4() {
                Shown::UpToDate | Shown::Upgraded => style::positive(),
                Shown::Failed => style::negative(),
                Shown::Available(_) => style::info(),
                _ => style::secondary_label(),
            }),
        )
        .child(
            text(move || {
                let m = &text_m;
                match s5() {
                    Shown::Checking => m.tr("ui.updates.checking"),
                    Shown::UpToDate => m.tr("ui.updates.up_to_date"),
                    Shown::Failed => m.tr("ui.updates.failed"),
                    Shown::Latest(v) => m.tr_args("ui.updates.latest", &[&version(&v)]),
                    Shown::Available(v) => format!("→ {}", version(&v)),
                    Shown::Upgrading => m.tr("ui.footer.core_upgrade.running"),
                    Shown::Upgraded => m.tr("ui.footer.core_upgrade.success"),
                    Shown::Idle => String::new(),
                }
            })
            .text_size(px(font::CAPTION))
            .text_color({
                let m = m.clone();
                move || if m.core_update_available() { style::info() } else { style::secondary_label() }
            }),
        )
}
