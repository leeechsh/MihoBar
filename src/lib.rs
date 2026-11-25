//! MihoBar: a lightweight native macOS menu bar client for mihomo, on NSH.
//! It draws on the ideas and design of ClashBar
//! (https://github.com/Sitoi/ClashBar, GPL-3.0); see README.md.

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
pub mod updates;
pub mod usage;
#[cfg(target_os = "macos")]
pub mod wifi;
pub mod widgets;
/// The location permission the Wi-Fi name needs (NSH's `location`,
/// on every platform).
pub mod wifi_types {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
    pub enum Authorization {
        #[default]
        NotDetermined,
        Authorized,
        Denied,
        Restricted,
    }

    impl Authorization {
        pub fn label_key(self) -> &'static str {
            match self {
                Self::Authorized => "ui.settings.ssid_strategy.status.authorized",
                Self::NotDetermined => "ui.settings.ssid_strategy.status.not_determined",
                Self::Denied => "ui.settings.ssid_strategy.status.denied",
                Self::Restricted => "ui.settings.ssid_strategy.status.restricted",
            }
        }
    }

    impl From<nsh_extra::location::Authorization> for Authorization {
        fn from(a: nsh_extra::location::Authorization) -> Self {
            use nsh_extra::location::Authorization as L;
            match a {
                L::WhenInUse | L::Always => Self::Authorized,
                L::Denied => Self::Denied,
                L::Restricted => Self::Restricted,
                _ => Self::NotDetermined,
            }
        }
    }
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

/// The advanced window's process: one normal window showing rules,
/// connections, logs and usage. Closing it quits the process, which gives
/// all its memory back.
fn advanced_app(launch: crate::model::AdvancedLaunch) -> impl IntoElement {
    use nsh_core::desktop::{Backdrop, TitleBarOptions, TitleBarStyle, WindowAttributes, WindowFrameStyle, WindowLevel, WindowPosition, Workspaces};
    let m = Model::new_advanced(launch);
    let mut a = WindowAttributes::default();
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
    // Kept inside the main screen.
    let screen = crate::geometry::visible_frame_at(f32::NAN);
    let (w, h) = (crate::ui_manager::WIDTH.min(screen.w - 40.0), crate::ui_manager::HEIGHT.min(screen.h - 40.0));
    let title_m = m.clone();
    nsh_core::element::desktop::window()
        .id("advanced")
        .kind(WindowKind::Normal)
        .title(move || title_m.tr("ui.manager.title"))
        .attributes(a)
        .default_size(size(Points(w), Points(h)))
        .default_position(WindowPosition::Center)
        .on_open_change(|change| {
            if !change.open {
                nsh_core::element::desktop::quit();
            }
        })
        .content(move || {
            current_window().focus();
            view().flex_1().child(crate::ui_manager::manager(m.clone()))
        })
}

/// The menu bar panel's process.
fn panel_app() -> impl IntoElement {
    let m = Model::new();
    // Update checks run in the installed app (not in tests or dev runs).
    if crate::model::bundled_executable().is_some() {
        m.start_update_checks();
    }
    // The panel is a window of its own (NSH's window model v2: the app's
    // root never shows), open while `panel_visible` holds, its views made
    // as it opens. Its ref, while open, and where it opens next.
    let panel_ref: std::rc::Rc<std::cell::RefCell<Option<nsh_core::element::desktop::WindowRef>>> = Default::default();
    let panel_origin: std::rc::Rc<std::cell::Cell<Option<(f32, f32)>>> = Default::default();

    // Shows the panel under `icon` (centered, ClashBar's PanelGeometry),
    // or hides it.
    {
        let m2 = m.clone();
        m.set_panel_hide(std::rc::Rc::new(move || {
            m2.panel_visible.set(false);
            m2.side_popover.set(None);
        }));
    }
    let toggle_panel = {
        let m = m.clone();
        let panel_origin = panel_origin.clone();
        std::rc::Rc::new(move |icon: Option<crate::geometry::Rect>| {
            if m.panel_visible.get_untracked() {
                m.panel_visible.set(false);
                m.side_popover.set(None);
            } else {
                if let Some(icon) = icon {
                    panel_origin.set(Some(crate::geometry::panel_origin(icon, crate::style::PANEL_WIDTH, 4.0)));
                }
                // It opens on the node tab every time.
                m.tab.set(crate::model::RootTab::Proxy);
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
                set_tray(tray().icon(icon).tooltip("MihoBar").on_click(move |click| {
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
        let panel_ref = panel_ref.clone();
        let side_window = side_window.clone();
        std::rc::Rc::new(move || {
            let (m, panel_ref, side_window) = (m.clone(), panel_ref.clone(), side_window.clone());
            // The focus moving between the two windows passes through
            // neither: look once it has landed.
            set_timeout(std::time::Duration::from_millis(120), move || {
                // No state (a window not on a screen, as in headless
                // tests): the focus is not known, so the panel stays.
                let Some(panel_state) = panel_ref.borrow().as_ref().and_then(|p| p.state()) else { return };
                let side_focused = side_window.borrow().as_ref().is_some_and(|w| w.state().is_some_and(|s| s.focused));
                if panel_state.focused || side_focused || m.settings.with_untracked(|s| s.pinned) {
                    return;
                }
                m.panel_visible.set(false);
                m.side_popover.set(None);
            });
        })
    };
    // Development and tests: MIHOBAR_SHOW_PANEL=1 shows the panel at
    // launch (the test runner cannot click the menu bar item).
    if std::env::var_os("MIHOBAR_SHOW_PANEL").is_some_and(|v| v == "1") {
        // Where a menu bar item would put it: the top right of the main
        // screen (off the management window, centered).
        let screen = crate::geometry::visible_frame_at(f32::NAN);
        toggle_panel(Some(crate::geometry::Rect { x: screen.x + screen.w - 60.0, y: screen.y - 22.0, w: 22.0, h: 22.0 }));
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
        let host = panel_ref.clone();
        let open_m = m.clone();
        let close_m = m.clone();
        let refocus = panel_ref.clone();
        let side_window = side_window.clone();
        let hide_unless_focused = hide_unless_focused.clone();
        nsh_core::element::desktop::window()
            .id("side")
            .kind(WindowKind::Popup)
            .parent("panel")
            .attributes({
                let mut a = nsh_core::desktop::WindowAttributes::default();
                a.focusable = Some(true);
                a.close_on_blur = Some(false);
                a.close_on_escape = Some(true);
                // Its surface is rounded (see `style::window_surface`).
                a.transparent = Some(true);
                a.backdrop = Some(nsh_core::desktop::Backdrop::None);
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
                        if let Some(panel) = refocus.borrow().as_ref() {
                            panel.focus();
                        }
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
                        let host_rect = host.borrow().as_ref().and_then(|h| h.state()).map_or(
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

    // The panel: a rounded material over a transparent, frameless window
    // floating on every workspace (a menu bar popover). It closes as it
    // hides, dropping all its views (ClashBar's `unloadPopoverContent`).
    let panel = {
        let open_m = m.clone();
        let close_m = m.clone();
        let m = m.clone();
        let panel_ref = panel_ref.clone();
        let hide_unless_focused = hide_unless_focused.clone();
        let mut a = nsh_core::desktop::WindowAttributes::default();
        a.title = Some("MihoBar".to_owned());
        a.focusable = Some(true);
        a.movable = Some(false);
        a.close_on_press = Some(false);
        a.close_on_escape = Some(false);
        // It hides itself when the focus leaves it and the side popover.
        a.close_on_blur = Some(false);
        a.transparent = Some(true);
        a.backdrop = Some(nsh_core::desktop::Backdrop::None);
        a.shadow = Some(true);
        nsh_core::element::desktop::window()
            .id("panel")
            .kind(WindowKind::Popup)
            .attributes(a)
            .default_size(size(Points(crate::style::PANEL_WIDTH), Points(560.0)))
            .open(move || open_m.panel_visible.get())
            .on_open_change(move |change| {
                if !change.open {
                    close_m.panel_visible.set(false);
                    close_m.side_popover.set(None);
                }
            })
            .content(move || {
                let me = current_window();
                *panel_ref.borrow_mut() = Some(me.clone());
                if let Some((x, y)) = panel_origin.get() {
                    me.set_bounds(WindowFrame { x: Some(Points(x)), y: Some(Points(y)), ..Default::default() });
                }
                me.focus();
                {
                    let hide = hide_unless_focused.clone();
                    me.on_focus_change(move |focused| {
                        if !focused {
                            hide();
                        }
                    });
                }
                {
                    let panel_ref = panel_ref.clone();
                    cleanup(move || {
                        panel_ref.borrow_mut().take();
                    });
                }
                crate::style::window_surface(view())
                    .flex_1()
                    .bg(crate::style::menu_background())
                    .child(ui::panel(m.clone(), me))
            })
    };

    view().child(panel).child(side).child(manager)
}
