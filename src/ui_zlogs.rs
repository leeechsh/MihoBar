//! The advanced window's logs page (zashboard's): the core's and
//! Mihobar's log lines at the page's own level, newest first (the last
//! lines of their files, then live), by source, level or type, searched
//! with a regex, some hidden by another; download, pause, clear.

use nsh_core::element::desktop::MenuItem;
use nsh_core::element::input;
use nsh_core::prelude::*;

use crate::conns::{self, ZLog, ZLogSource};
use crate::icons;
use crate::model::Model;
use crate::style::z;
use crate::ui_zconns::{icon_button, search_field, select_menu, sheet};

/// The levels the core streams (zashboard's `supportedLogLevels`).
const LEVELS: [&str; 5] = ["debug", "info", "warning", "error", "silent"];

fn level_color(level: &str) -> AdaptiveColor {
    match level {
        "trace" => z::success(),
        "debug" => z::text(0.55),
        "info" => z::info(),
        "warning" => z::warning(),
        _ => z::error(),
    }
}

pub fn logs_page(m: Model) -> impl IntoElement {
    let search = signal(String::new());
    let type_filter = signal(String::new());
    let settings_open = signal(false);
    let list_m = m.clone();
    let shown = computed(move || {
        let (hide_on, hide, source) = list_m.z_settings.with(|s| (s.log_hide_enabled, s.log_hide_regex.clone(), s.log_source.clone()));
        let search = search.get();
        let kind = type_filter.get();
        list_m.z_logs.with(|logs| conns::filter_logs(logs, &source, &kind, &search, hide_on.then_some(hide.as_str())))
    });

    view()
        .flex_1()
        .pt(px(30.0))
        .child(controls(m.clone(), search, type_filter, settings_open))
        .child(log_list(m.clone(), shown))
        .child(when(move || settings_open.get(), move || settings_sheet(m.clone(), settings_open)))
}

/// The source filter's label for `key` ("", "app" or "core").
fn source_label(key: &str) -> &'static str {
    match key {
        "app" => "z.source_app",
        "core" => "z.source_core",
        _ => "z.source_all",
    }
}

fn source_tag(m: &Model, source: ZLogSource) -> String {
    m.tr(match source {
        ZLogSource::App => "z.source_app",
        ZLogSource::Core => "z.source_core",
    })
}

fn controls(m: Model, search: Signal<String>, type_filter: Signal<String>, settings_open: Signal<bool>) -> impl IntoElement {
    let level_m = m.clone();
    let level_items_m = m.clone();
    let level = select_menu(
        move || level_m.z_settings.with(|s| s.log_level.clone()),
        110.0,
        move || {
            let m = level_items_m.clone();
            let current = m.z_settings.with_untracked(|s| s.log_level.clone());
            LEVELS
                .iter()
                .map(|&l| {
                    let m = m.clone();
                    // A new level opens the stream again (the page's own).
                    MenuItem::new(l).checked(current == l).on_select(move || m.z_settings.update(|s| s.log_level = l.to_owned()))
                })
                .collect()
        },
    );
    let source_m = m.clone();
    let source_items_m = m.clone();
    let sources = select_menu(
        move || {
            let key = source_m.z_settings.with(|s| s.log_source.clone());
            source_m.tr(source_label(&key))
        },
        120.0,
        move || {
            let m = source_items_m.clone();
            let current = m.z_settings.with_untracked(|s| s.log_source.clone());
            ["", "app", "core"]
                .iter()
                .map(|&key| {
                    let m2 = m.clone();
                    MenuItem::new(m.tr(source_label(key)))
                        .checked(current == key)
                        .on_select(move || m2.z_settings.update(|s| s.log_source = key.to_owned()))
                })
                .collect()
        },
    );
    let type_m = m.clone();
    let type_items_m = m.clone();
    let types = select_menu(
        move || {
            let t = type_filter.get();
            if t.is_empty() { type_m.tr("z.all") } else { t }
        },
        130.0,
        move || {
            let m = type_items_m.clone();
            let (mut levels, mut kinds): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
            m.z_logs.with_untracked(|logs| {
                for l in logs.iter() {
                    if !levels.contains(&l.level) {
                        levels.push(l.level.clone());
                    }
                    let k = l.kind().to_owned();
                    if !k.is_empty() && !kinds.contains(&k) {
                        kinds.push(k);
                    }
                }
            });
            levels.sort_by_key(|l| LEVELS.iter().position(|x| x == l).unwrap_or(99));
            kinds.sort();
            let current = type_filter.get_untracked();
            let mut items = vec![MenuItem::new(m.tr("z.all")).checked(current.is_empty()).on_select(move || type_filter.set(String::new()))];
            let section = |title: String, values: Vec<String>| {
                let mut out = vec![MenuItem::separator(), MenuItem::new(title).disabled(true)];
                out.extend(values.into_iter().map(|v| {
                    let on = current == v;
                    let value = v.clone();
                    MenuItem::new(v).checked(on).on_select(move || type_filter.set(value.clone()))
                }));
                out
            };
            items.extend(section(m.tr("z.log_level"), levels));
            items.extend(section(m.tr("z.log_type"), kinds));
            items
        },
    );
    let search = search_field(m.tr("z.search_regex"), search, view().into_element(), 260.0);
    let (s_m, dl_m, dl_l, hide_m, hide_l, pause_m, pause_l, clear_m, clear_l) =
        (m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(8.0))
        .px(px(12.0))
        .pb(px(8.0))
        .child(level)
        .child(sources)
        .child(types)
        .child(search)
        .child(view().flex_1())
        .child(icon_button(icons::z_tools, move || s_m.tr("z.log_settings"), move || settings_open.set(true)))
        .child(icon_button(icons::z_download, move || dl_l.tr("z.download_logs"), move || download(&dl_m)))
        .child(icon_button(
            {
                let m = hide_m.clone();
                move || if m.z_settings.with(|s| s.log_hide_enabled) { icons::z_link_off() } else { icons::z_link() }
            },
            move || hide_l.tr(if hide_l.z_settings.with(|s| s.log_hide_enabled) { "z.show_log" } else { "z.hide_log" }),
            move || hide_m.z_settings.update(|s| s.log_hide_enabled = !s.log_hide_enabled),
        ))
        .child(icon_button(
            {
                let m = pause_m.clone();
                move || if m.z_logs_paused.get() { icons::z_play() } else { icons::z_pause() }
            },
            move || pause_l.tr(if pause_l.z_logs_paused.get() { "z.resume" } else { "z.pause" }),
            move || pause_m.z_logs_paused.update(|p| *p = !*p),
        ))
        .child(icon_button(icons::z_xmark, move || clear_l.tr("z.clear_logs"), move || clear_m.z_logs.set(std::rc::Rc::new(Vec::new()))))
}

/// Saves the logs, oldest first, where the user chooses (zashboard
/// downloads `YYYY-MM-DD HH-mm-ss.log`), then shows the file: NSH's save
/// dialog and file manager, on every platform.
fn download(m: &Model) {
    let (app, core) = (source_tag(m, ZLogSource::App), source_tag(m, ZLogSource::Core));
    let text: String = m.z_logs.with_untracked(|logs| {
        logs.iter()
            .rev()
            .map(|l| {
                let p = crate::format::local_parts(l.at);
                let source = if l.source == ZLogSource::App { &app } else { &core };
                format!("{:04}-{:02}-{:02} {} [{source}] {} {}\n", p.0, p.1, p.2, l.time, l.level.to_uppercase(), l.payload)
            })
            .collect()
    });
    let p = crate::format::local_parts(crate::format::now_secs());
    let name = format!("{:04}-{:02}-{:02} {:02}-{:02}-{:02}.log", p.0, p.1, p.2, p.3, p.4, p.5);
    let title = m.tr("z.download_logs");
    spawn(async move {
        let mut dialog = nsh_core::file_dialog::SaveDialog::new().file_name(name);
        dialog.title = Some(title);
        dialog.filters = vec![nsh_core::file_dialog::FileFilter::new("Log", ["log"])];
        let Ok(Some(path)) = pick_save_path(dialog).await else { return };
        if std::fs::write(&path, text).is_ok() {
            let _ = reveal_path(path).await;
        }
    });
}

fn log_list(m: Model, shown: Memo<Vec<ZLog>>) -> impl IntoElement {
    let (app, core) = (source_tag(&m, ZLogSource::App), source_tag(&m, ZLogSource::Core));
    let list = virtual_list(move || shown.with(Vec::len), move |at: nsh_core::element::Position| {
        let item = computed(move || shown.with(|l| l.get(at.index()).cloned()));
        let (app, core) = (app.clone(), core.clone());
        view().child(dynamic(move || match item.get() {
            Some(log) => {
                let tag = if log.source == ZLogSource::App { app.clone() } else { core.clone() };
                log_row(log, tag).into_element()
            }
            None => view().into_element(),
        }))
    })
    .estimated_item_size(px(56.0))
    .flex_1();
    view()
        .flex_1()
        .mx(px(12.0))
        .mb(px(12.0))
        .bg(z::card())
        .rounded(px(12.0))
        .border_1()
        .border_color(z::base300(0.35))
        .shadow_xs()
        .overflow_hidden()
        .child(list)
}

/// A line (zashboard's `LogsCard`): its number, source, level and time
/// over the text.
fn log_row(log: ZLog, tag: String) -> impl IntoElement {
    let app = log.source == ZLogSource::App;
    let seq = format!("{:02}", log.seq);
    view()
        .gap(px(4.0))
        .px(px(12.0))
        .py(px(10.0))
        .border_b_1()
        .border_color(z::base300(0.3))
        .hover(|s| s.bg(z::base200(0.4)))
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(8.0))
                .child(text(seq).text_size(px(12.0)).text_color(z::text(0.4)))
                .child(
                    text(tag)
                        .text_size(px(10.0))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(if app { z::info() } else { z::text(0.55) })
                        .px(px(6.0))
                        .py(px(1.0))
                        .rounded(px(5.0))
                        .bg(if app { z::info().opacity(0.14) } else { z::base300(0.45) }),
                )
                .child(text(log.level.to_uppercase()).text_size(px(11.0)).text_color(level_color(&log.level)))
                .child(view().flex_1())
                .child(text(log.time).text_size(px(12.0)).text_color(z::text(0.4))),
        )
        .child(text(log.payload).text_size(px(13.0)).text_color(z::text(1.0)))
}

fn settings_sheet(m: Model, open: Signal<bool>) -> impl IntoElement {
    let retention = signal(m.z_settings.with_untracked(|s| s.log_retention.to_string()));
    {
        let m = m.clone();
        effect(move || {
            if let Ok(n) = retention.get().trim().parse::<usize>() {
                untracked(|| m.z_settings.update(|s| s.log_retention = n.clamp(1, 100_000)));
            }
        });
    }
    let regex = signal(m.z_settings.with_untracked(|s| s.log_hide_regex.clone()));
    {
        let m = m.clone();
        effect(move || {
            let r = regex.get();
            untracked(|| m.z_settings.update(|s| s.log_hide_regex = r));
        });
    }
    let hide_m = m.clone();
    let row = |label: String, control: AnyElement| {
        view()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .min_h(px(44.0))
            .child(text(label).flex_1().text_size(px(13.0)).text_color(z::text(1.0)))
            .child(control)
    };
    sheet(
        m.tr("z.log_settings"),
        480.0,
        300.0,
        move || open.set(false),
        Vec::new(),
        view()
            .px(px(20.0))
            .py(px(12.0))
            .child(row(
                m.tr("z.log_retention"),
                view()
                    .w(px(120.0))
                    .h(px(32.0))
                    .px(px(10.0))
                    .justify_center()
                    .rounded(px(8.0))
                    .border_1()
                    .border_color(z::base300(0.7))
                    .child(input().value(move || retention.get()).on_change(move |v: &str| retention.set(v.to_owned())).text_size(px(13.0)).text_color(z::text(1.0)))
                    .into_element(),
            ))
            .child(row(m.tr("z.hide_log_regex"), search_field(String::new(), regex, view().into_element(), 220.0).into_element()))
            .child(row(
                m.tr("z.hide_log"),
                view()
                    .on_press(move || hide_m.z_settings.update(|s| s.log_hide_enabled = !s.log_hide_enabled))
                    .child(crate::widgets::switch({
                        let m = m.clone();
                        move || m.z_settings.with(|s| s.log_hide_enabled)
                    }, || false))
                    .into_element(),
            )),
    )
}

