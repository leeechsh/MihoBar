//! Mihobar: a native macOS menu bar client for mihomo, on NSH.
//! A port of ClashBar (https://github.com/Sitoi/ClashBar), GPL-3.0.

pub mod api;
pub mod config;
pub mod conns;
pub mod core;
pub mod data;
pub mod features;
pub mod format;
pub mod geometry;
pub mod i18n;
pub mod icons;
pub mod model;
#[cfg(target_os = "macos")]
pub mod netmon;
pub mod paths;
pub mod proxy;
pub mod settings;
pub mod settings_core;
pub mod stream;
pub mod subscriptions;
#[cfg(target_os = "macos")]
pub mod statusitem;
#[cfg(target_os = "macos")]
pub mod sysproxy;
pub mod style;
pub mod ui;
pub mod ui_proxy;
pub mod ui_settings;
pub mod ui_manager;
pub mod ui_tabs;
pub mod ui_zconns;
pub mod ui_zlogs;
pub mod ui_zrules;
pub mod usage;
#[cfg(target_os = "macos")]
pub mod wifi;
/// The Wi-Fi permission states (also where there is no Wi-Fi module).
pub mod widgets;
pub mod wifi_types {
    #[cfg(target_os = "macos")]
    pub use crate::wifi::Authorization;
}

use nsh_core::element::desktop::{current_window, set_tray, tray};
use nsh_core::prelude::*;

use nsh_core::desktop::{WindowFrame, WindowKind};

use crate::model::Model;

/// The app's UI set (`nsh.json` `"rust"`).
#[native_module("mihobar/native")]
fn native(registry: &mut Registry) {
    // Ships `assets/` (the bundled mihomo core and the default config).
    registry.assets(assets!());
    registry.register::<App>();
}

/// The app's root: the panel is the main window, hidden at launch
/// (`nsh.json` `window.defaultVisible: false`); the menu bar item shows
/// and hides it.
#[component]
pub fn App() -> impl IntoElement {
    // Started by the panel's process for the advanced window: that window
    // alone, and the process ends with it.
    match crate::model::AdvancedLaunch::from_env() {
        Some(launch) => advanced_app(launch).into_element(),
        None => panel_app().into_element(),
    }
}

/// The advanced window's process: its main window (the panel's, from
/// `nsh.json`) made a normal window showing rules, connections and logs.
/// Closing it quits the process, which gives all its memory back.
fn advanced_app(launch: crate::model::AdvancedLaunch) -> impl IntoElement {
    use nsh_core::desktop::{Backdrop, TitleBarOptions, TitleBarStyle, WindowAttributes, WindowFrameStyle, WindowLevel, Workspaces};
    let m = Model::new_advanced(launch);
    let win = current_window();
    let mut a = WindowAttributes::default();
    a.title = Some(m.tr("ui.manager.title"));
    let mut bar = TitleBarOptions::default();
    bar.style = TitleBarStyle::Hidden;
    a.title_bar = Some(bar);
    a.frame = Some(WindowFrameStyle::Default);
    a.min_size = Some(Some(size(Points(crate::ui_manager::MIN_WIDTH), Points(crate::ui_manager::MIN_HEIGHT))));
    a.resizable = Some(true);
    a.movable = Some(true);
    a.minimizable = Some(true);
    a.maximizable = Some(true);
    a.closable = Some(true);
    a.close_on_press = Some(true);
    a.level = Some(WindowLevel::Normal);
    a.workspaces = Some(Workspaces::Current);
    a.backdrop = Some(Backdrop::None);
    a.transparent = Some(false);
    a.taskbar = Some(true);
    win.set(a);
    // Centered on the main screen, kept inside it.
    let screen = crate::geometry::visible_frame_at(f32::NAN);
    let (w, h) = (crate::ui_manager::WIDTH.min(screen.w - 40.0), crate::ui_manager::HEIGHT.min(screen.h - 40.0));
    win.set_bounds(WindowFrame {
        x: Some(Points(screen.x + (screen.w - w) / 2.0)),
        y: Some(Points(screen.y + (screen.h - h) / 2.0)),
        width: Some(Points(w)),
        height: Some(Points(h)),
        ..Default::default()
    });
    win.focus();
    win.on_close_press(|_| nsh_core::element::desktop::quit());
    view().flex_1().child(crate::ui_manager::manager(m))
}

/// The menu bar panel's process.
fn panel_app() -> impl IntoElement {
    let m = Model::new();
    let win = current_window();

    // Shows the panel under `icon` (centered, ClashBar's PanelGeometry),
    // or hides it.
    let toggle_panel = {
        let m = m.clone();
        let win = win.clone();
        std::rc::Rc::new(move |icon: Option<crate::geometry::Rect>| {
            let visible = win.state().is_some_and(|s| s.visible);
            if visible {
                win.hide();
                m.panel_visible.set(false);
                m.side_popover.set(None);
            } else {
                if let Some(icon) = icon {
                    let (x, y) = crate::geometry::panel_origin(icon, crate::style::PANEL_WIDTH, 4.0);
                    win.set_bounds(WindowFrame { x: Some(Points(x)), y: Some(Points(y)), ..Default::default() });
                }
                win.focus();
                m.panel_visible.set(true);
            }
        })
    };

    // The menu bar item (ClashBar's `StatusItemController`): icon only
    // through NSH's tray; with the speed, an item drawn by hand.
    {
        let m = m.clone();
        let toggle = toggle_panel.clone();
        let native_shown = std::rc::Rc::new(std::cell::Cell::new(false));
        effect(move || {
            let running = m.is_live();
            let mode = m.settings.with(|s| s.status_bar_display_mode);
            if mode == crate::settings::StatusBarDisplayMode::IconOnly {
                if native_shown.replace(false) {
                    #[cfg(target_os = "macos")]
                    let _ = run_on_main_thread(crate::statusitem::hide);
                }
                let icon = if !running {
                    crate::icons::status_sleep()
                } else if m.tun_enabled() {
                    crate::icons::status_tun()
                } else {
                    crate::icons::status_run()
                };
                let toggle = toggle.clone();
                set_tray(tray().icon(icon).tooltip("Mihobar").on_click(move |click| {
                    toggle(click.bounds.map(crate::geometry::Rect::from_bounds));
                }));
                return;
            }
            set_tray(None);
            native_shown.set(true);
            let traffic = m.traffic.get();
            let (up, down) = if running {
                (
                    format!("{}↑", crate::format::compact_menu_bar_rate(traffic.up)),
                    format!("{}↓", crate::format::compact_menu_bar_rate(traffic.down)),
                )
            } else {
                ("0K↑".to_owned(), "0K↓".to_owned())
            };
            #[cfg(target_os = "macos")]
            {
                let display = crate::statusitem::Display {
                    show_icon: mode == crate::settings::StatusBarDisplayMode::IconAndSpeed,
                    running,
                    tun: m.tun_enabled(),
                    up,
                    down,
                };
                let _ = run_on_main_thread(move || crate::statusitem::show(&display));
            }
        });
    }
    #[cfg(target_os = "macos")]
    {
        let mut clicks = crate::statusitem::clicks();
        let toggle = toggle_panel.clone();
        crate::model::run_root(async move {
            while let Some(icon) = clicks.recv().await {
                toggle(Some(icon));
            }
        });
    }

    // The panel closes when the focus leaves it and the popover beside
    // it (which takes the focus for its text fields), unless pinned.
    let side_window: std::rc::Rc<std::cell::RefCell<Option<nsh_core::element::desktop::WindowRef>>> = Default::default();
    let hide_unless_focused: std::rc::Rc<dyn Fn()> = {
        let m = m.clone();
        let panel = win.clone();
        let side_window = side_window.clone();
        std::rc::Rc::new(move || {
            let (m, panel, side_window) = (m.clone(), panel.clone(), side_window.clone());
            // The focus moving between the two windows passes through
            // neither: look once it has landed.
            set_timeout(std::time::Duration::from_millis(120), move || {
                // No state (a window not on a screen, as in headless
                // tests): the focus is not known, so the panel stays.
                let Some(panel_state) = panel.state() else { return };
                let side_focused = side_window.borrow().as_ref().is_some_and(|w| w.state().is_some_and(|s| s.focused));
                if panel_state.focused || side_focused || m.settings.with_untracked(|s| s.pinned) {
                    return;
                }
                panel.hide();
                m.panel_visible.set(false);
                m.side_popover.set(None);
            });
        })
    };
    {
        let hide = hide_unless_focused.clone();
        win.on_focus_change(move |focused| {
            if !focused {
                hide();
            }
        });
    }
    {
        let hide = win.clone();
        let m = m.clone();
        win.on_close_press(move |_| {
            hide.hide();
            m.panel_visible.set(false);
        });
    }

    // Development and tests: MIHOBAR_SHOW_PANEL=1 shows the panel at
    // launch (the test runner cannot click the menu bar item).
    if std::env::var_os("MIHOBAR_SHOW_PANEL").is_some_and(|v| v == "1") {
        let win = win.clone();
        let m = m.clone();
        set_timeout(std::time::Duration::from_millis(0), move || {
            // Where a menu bar item would put it: the top right of the
            // main screen (off the management window, centered).
            let screen = crate::geometry::visible_frame_at(f32::NAN);
            let icon = crate::geometry::Rect { x: screen.x + screen.w - 60.0, y: screen.y - 22.0, w: 22.0, h: 22.0 };
            let (x, y) = crate::geometry::panel_origin(icon, crate::style::PANEL_WIDTH, 4.0);
            win.set_bounds(WindowFrame { x: Some(Points(x)), y: Some(Points(y)), ..Default::default() });
            win.focus();
            m.panel_visible.set(true);
        });
    }

    // Quit (⌘Q, the system's logout or shutdown) stops the core first.
    {
        let mut options = nsh_core::desktop::QuitRequestOptions::default();
        options.system = true;
        nsh_core::element::desktop::set_quit_request_options(options);
    }
    {
        let m = m.clone();
        nsh_core::element::desktop::on_quit_request(move || {
            let m = m.clone();
            crate::model::run_root(async move { m.quit().await });
        });
    }

    // The popover attached beside the panel (a group's nodes, the
    // configs, the forms): it takes the focus for its text fields; the
    // panel stays while either window has it.
    let side = {
        let m = m.clone();
        let host = win.clone();
        let open_m = m.clone();
        let close_m = m.clone();
        let refocus = win.clone();
        let side_window = side_window.clone();
        let hide_unless_focused = hide_unless_focused.clone();
        nsh_core::element::desktop::window()
            .id("side")
            .kind(WindowKind::Popup)
            .parent("main")
            .attributes({
                let mut a = nsh_core::desktop::WindowAttributes::default();
                a.focusable = Some(true);
                a.close_on_blur = Some(false);
                a.close_on_escape = Some(true);
                a.backdrop = Some(nsh_core::desktop::Backdrop::Popover);
                a.shadow = Some(true);
                a
            })
            .default_size(size(Points(crate::ui_proxy::SIDE_WIDTH), Points(300.0)))
            .open(move || open_m.side_popover.with(|p| p.is_some()))
            .on_open_change(move |change| {
                if !change.open {
                    close_m.side_popover.set(None);
                    // The focus goes back to the panel, so a click
                    // elsewhere still closes it.
                    if close_m.panel_visible.get_untracked() {
                        refocus.focus();
                    }
                }
            })
            .content(move || {
                let me = current_window();
                *side_window.borrow_mut() = Some(me.clone());
                {
                    let hide = hide_unless_focused.clone();
                    me.on_focus_change(move |focused| {
                        if !focused {
                            hide();
                        }
                    });
                }
                let m = m.clone();
                let host = host.clone();
                // Placed beside the panel, level with its row.
                effect({
                    let m = m.clone();
                    move || {
                        let Some(popover) = m.side_popover.get() else { return };
                        let host_rect = host.state().map_or(
                            crate::geometry::Rect { x: 0.0, y: 0.0, w: crate::style::PANEL_WIDTH, h: 560.0 },
                            |s| crate::geometry::Rect::from_bounds(s.bounds),
                        );
                        let h = crate::ui_proxy::side_height(&m, &popover.kind);
                        let frame = crate::geometry::attached_frame(
                            host_rect,
                            popover.anchor_mid_y,
                            crate::ui_proxy::SIDE_WIDTH,
                            h,
                        );
                        me.set_bounds(WindowFrame {
                            x: Some(Points(frame.x)),
                            y: Some(Points(frame.y)),
                            width: Some(Points(frame.w)),
                            height: Some(Points(frame.h)),
                            ..Default::default()
                        });
                    }
                });
                crate::widgets::pop_in(crate::ui_proxy::side_content(m))
            })
    };

    // The management window (rules, connections, logs, advanced
    // settings): made as it opens, all of it dropped as it closes.
    let manager = {
        let open_m = m.clone();
        let close_m = m.clone();
        let title_m = m.clone();
        let m = m.clone();
        nsh_core::element::desktop::window()
            .id("manager")
            .kind(WindowKind::Normal)
            .title(move || title_m.tr("ui.manager.title"))
            .attributes({
                let mut a = nsh_core::desktop::WindowAttributes::default();
                a.min_size = Some(Some(size(Points(crate::ui_manager::MIN_WIDTH), Points(crate::ui_manager::MIN_HEIGHT))));
                // The sidebar runs up under the window's controls.
                let mut bar = nsh_core::desktop::TitleBarOptions::default();
                bar.style = nsh_core::desktop::TitleBarStyle::Hidden;
                a.title_bar = Some(bar);
                a
            })
            .default_size(size(Points(crate::ui_manager::WIDTH), Points(crate::ui_manager::HEIGHT)))
            .restore_bounds(true)
            .open(move || open_m.manager_open.get())
            .on_open_change(move |change| {
                if !change.open {
                    close_m.set_manager_window(None);
                    close_m.manager_open.set(false);
                }
            })
            .content(move || {
                let me = current_window();
                m.set_manager_window(Some(me.clone()));
                me.focus();
                crate::ui_manager::manager(m.clone())
            })
    };

    // The panel's views exist only while it shows (ClashBar's
    // `unloadPopoverContent`): a hidden panel holds no views.
    let shown = m.clone();
    let content_m = m.clone();
    // The panel's white material covers the whole window.
    view()
        .flex_1()
        .bg(crate::style::menu_background())
        .child(when(move || shown.panel_visible.get(), move || ui::panel(content_m.clone(), win.clone())))
        .child(side)
        .child(manager)
}
