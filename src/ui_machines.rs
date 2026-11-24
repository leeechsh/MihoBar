//! The remote machine manager and editor (ClashBar's
//! `RemoteMachineManagerView`), shown beside the panel.

use std::rc::Rc;

use nsh_core::element::{ImageSrc, ReadSignal, input};
use nsh_core::prelude::*;

use crate::icons;
use crate::model::{Model, SideKind, SidePopover};
use crate::remote::{MachineDraft, MachineStatus, RemoteMachine};
use crate::style::{self, font};

fn icon(src: String, size: f32, color: impl Fn() -> AdaptiveColor + 'static) -> impl IntoElement {
    image(ImageSrc::from(src)).w(px(size)).h(px(size)).tint_color(move || color())
}

fn status_tint(status: &MachineStatus) -> AdaptiveColor {
    match status {
        MachineStatus::Connected(_) => style::positive(),
        MachineStatus::Failed(_) => style::negative(),
        MachineStatus::Checking => style::warning(),
        MachineStatus::Unknown => style::tertiary_label(),
    }
}

/// Opens the manager or the editor where the side popover already is.
fn show(m: &Model, kind: SideKind) {
    let anchor = m.side_popover.with_untracked(|p| p.as_ref().map(|p| p.anchor_mid_y)).unwrap_or(0.0);
    m.side_popover.set(Some(SidePopover { kind, anchor_mid_y: anchor }));
}

pub fn open_editor(m: &Model, machine: Option<&RemoteMachine>) {
    m.machine_draft.set(MachineDraft::from_machine(machine));
    show(m, SideKind::MachineEditor(machine.map(|m| m.id.clone())));
}

fn header(m: &Model, title: impl Fn() -> String + 'static, back: bool) -> impl IntoElement {
    let close_m = m.clone();
    let label_m = m.clone();
    view()
        .child(
            view()
                .flex_row()
                .items_center()
                .gap(px(style::SPACE6))
                .px(px(style::SPACE4))
                .pb(px(style::SPACE4))
                .child(icon(icons::network(), 13.0, style::info))
                .child(text(title).flex_1().truncate().text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::primary_label()))
                .child(
                    view()
                        .w(px(20.0))
                        .h(px(20.0))
                        .items_center()
                        .justify_center()
                        .rounded(px(10.0))
                        .hover(|s| s.bg(style::fill(0.08)))
                        .label(move || label_m.tr(if back { "ui.action.cancel" } else { "ui.action.close" }))
                        .on_press(move || {
                            if back {
                                show(&close_m, SideKind::Machines);
                            } else {
                                close_m.side_popover.set(None);
                            }
                        })
                        .child(icon(if back { icons::chevron_left() } else { icons::xmark() }, 11.0, style::secondary_label)),
                ),
        )
        .child(view().h(px(0.65)).bg(style::separator()).mb(px(style::SPACE4)))
}

fn row_fill(active: bool) -> AdaptiveColor {
    if active { style::accent(0.14) } else { style::fill(0.04) }
}

pub fn machines_list(m: Model) -> impl IntoElement {
    let title_m = m.clone();
    let list_m = m.clone();
    let add_m = m.clone();
    let add_label = m.clone();
    view()
        .flex_1()
        .child(header(&m, move || title_m.tr("ui.machine.manage"), false))
        .child(
            scroll().flex_1().child(
                view()
                    .gap(px(style::SPACE6))
                    .child(local_row(m.clone()))
                    .child(each(
                        move || list_m.settings.with(|s| s.remote_machines.clone()),
                        |machine: &RemoteMachine| machine.clone(),
                        {
                            let m = m.clone();
                            move |machine: ReadSignal<RemoteMachine>, _| machine_row(m.clone(), machine.get_untracked())
                        },
                    )),
            ),
        )
        .child(
            view()
                .flex_row()
                .items_center()
                .justify_center()
                .gap(px(style::SPACE4))
                .h(px(26.0))
                .mt(px(style::SPACE6))
                .rounded(px(style::CORNER_RADIUS))
                .bg(style::info())
                .on_press(move || open_editor(&add_m, None))
                .child(icon(icons::plus(), 11.0, style::white))
                .child(text(move || add_label.tr("ui.machine.add")).text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::white())),
        )
}

fn local_row(m: Model) -> impl IntoElement {
    let (active_m, fill_m, press_m, title_m, addr_m) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    view()
        .flex_row()
        .items_center()
        .gap(px(12.0))
        .p(px(10.0))
        .rounded(px(style::CORNER_RADIUS))
        .bg(move || row_fill(!fill_m.is_remote()))
        .on_press(move || {
            if press_m.is_remote_untracked() {
                let m = press_m.clone();
                m.side_popover.set(None);
                crate::model::run_root(async move { m.switch_to_machine(None).await });
            }
        })
        .child(icon(icons::desktop(), 16.0, style::info))
        .child(
            view()
                .flex_1()
                .gap(px(style::SPACE2))
                .child(text(move || title_m.tr("ui.machine.local")).text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::primary_label()))
                .child(text(move || addr_m.local_controller_display()).text_size(px(font::CAPTION)).text_color(style::secondary_label())),
        )
        .child(when(move || !active_m.is_remote(), || icon(icons::checkmark_circle_fill(), 14.0, style::info)))
}

fn machine_row(m: Model, machine: RemoteMachine) -> impl IntoElement {
    let id = machine.id.clone();
    let active = {
        let m = m.clone();
        let id = id.clone();
        move || m.remote.with(|r| r.as_ref().is_some_and(|r| r.id == id))
    };
    let status = {
        let m = m.clone();
        let id = id.clone();
        move || m.machine_status(&id)
    };
    let (press_m, edit_m, delete_m, edit_label, delete_label) = (m.clone(), m.clone(), m.clone(), m.clone(), m.clone());
    let (a1, a2, a3) = (active.clone(), active.clone(), active.clone());
    let (s1, s2, s3) = (status.clone(), status.clone(), status.clone());
    let (press_machine, edit_machine) = (machine.clone(), machine.clone());
    let delete_id = id.clone();
    let action = |src: String, tone: fn() -> AdaptiveColor, label: Rc<dyn Fn() -> String>, f: Rc<dyn Fn()>| {
        view()
            .w(px(26.0))
            .h(px(26.0))
            .items_center()
            .justify_center()
            .rounded(px(style::CORNER_RADIUS))
            .bg(style::fill(0.06))
            .hover(|s| s.bg(style::fill(0.12)))
            .label(move || label())
            .on_press(move || f())
            .child(icon(src, 12.0, tone))
    };
    view()
        .flex_row()
        .items_center()
        .gap(px(style::SPACE6))
        .p(px(10.0))
        .rounded(px(style::CORNER_RADIUS))
        .bg(move || row_fill(a1()))
        .child(
            view()
                .flex_1()
                .flex_row()
                .items_center()
                .gap(px(12.0))
                .opacity(move || if s1().is_connected() || a2() { 1.0_f32 } else { 0.7_f32 })
                .on_press(move || {
                    if s2().is_connected() && !a3() {
                        let m = press_m.clone();
                        let machine = press_machine.clone();
                        m.side_popover.set(None);
                        crate::model::run_root(async move { m.switch_to_machine(Some(machine)).await });
                    }
                })
                .child(icon(icons::network(), 16.0, style::info))
                .child(
                    view()
                        .flex_1()
                        .gap(px(style::SPACE2))
                        .child(text(machine.name.clone()).truncate().text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::primary_label()))
                        .child(
                            view()
                                .flex_row()
                                .items_center()
                                .gap(px(style::SPACE4))
                                .child(view().w(px(6.0)).h(px(6.0)).rounded(px(3.0)).bg(move || status_tint(&s3())))
                                .child(text(machine.display_address()).truncate().text_size(px(font::CAPTION)).text_color(style::secondary_label())),
                        )
                        .child(when(
                            {
                                let status = status.clone();
                                move || matches!(status(), MachineStatus::Failed(_))
                            },
                            {
                                let status = status.clone();
                                move || {
                                    let status = status.clone();
                                    text(move || match status() {
                                        MachineStatus::Failed(r) => r,
                                        _ => String::new(),
                                    })
                                    .truncate()
                                    .text_size(px(font::CAPTION))
                                    .text_color(style::negative())
                                }
                            },
                        )),
                ),
        )
        .child(when(active, || icon(icons::checkmark_circle_fill(), 14.0, style::info)))
        .child(action(
            icons::pencil(),
            style::secondary_label,
            Rc::new(move || edit_label.tr("ui.machine.edit")),
            Rc::new(move || open_editor(&edit_m, Some(&edit_machine))),
        ))
        .child(action(
            icons::trash(),
            style::negative,
            Rc::new(move || delete_label.tr("ui.action.delete")),
            Rc::new(move || delete_m.remove_machine(&delete_id)),
        ))
}

pub fn machine_editor(m: Model) -> impl IntoElement {
    let editing = {
        let m = m.clone();
        move || match m.side_popover.with(|p| p.as_ref().map(|p| p.kind.clone())) {
            Some(SideKind::MachineEditor(id)) => id,
            _ => None,
        }
    };
    let title_m = m.clone();
    let e1 = editing.clone();
    let draft = m.machine_draft;
    let field = move |title: Rc<dyn Fn() -> String>, placeholder: &'static str, get: fn(&MachineDraft) -> String, set: fn(&mut MachineDraft, String), secure: bool| {
        view()
            .flex_1()
            .gap(px(style::SPACE4))
            .child(text(move || title()).text_size(px(font::CAPTION)).font_weight(FontWeight::SEMIBOLD).text_color(style::secondary_label()))
            .child(
                input()
                    .h(px(24.0))
                    .px(px(style::SPACE6))
                    .rounded(px(5.0))
                    .border_1()
                    .border_color(style::separator())
                    .masked(secure)
                    .placeholder(placeholder)
                    .value(move || draft.with(get))
                    .on_change(move |v: &str| draft.update(|d| set(d, v.to_owned())))
                    .text_size(px(font::BODY))
                    .text_color(style::primary_label()),
            )
    };
    let tr = |m: &Model, key: &'static str| -> Rc<dyn Fn() -> String> {
        let m = m.clone();
        Rc::new(move || m.tr(key))
    };
    let (preview_m, save_m, save_label, https_m) = (m.clone(), m.clone(), m.clone(), m.clone());
    let save = move || {
        let d = draft.get_untracked();
        if !d.is_valid() {
            return;
        }
        let id = editing();
        let machine = d.to_machine(id.clone());
        if id.is_some() {
            save_m.update_machine(machine);
        } else {
            save_m.add_machine(machine);
        }
        show(&save_m, SideKind::Machines);
    };
    view()
        .flex_1()
        .child(header(&m, move || title_m.tr(if e1().is_some() { "ui.machine.edit" } else { "ui.machine.add" }), true))
        .child(
            view()
                .gap(px(style::SPACE4))
                .mb(px(style::SPACE8))
                .child(
                    text({
                        let m = m.clone();
                        move || {
                            let name = draft.with(|d| d.name.trim().to_owned());
                            if name.is_empty() { m.tr("ui.machine.field.name") } else { name }
                        }
                    })
                    .truncate()
                    .text_size(px(font::BODY))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(style::primary_label()),
                )
                .child(
                    text(move || {
                        let (scheme, host, port) = draft.with(|d| d.preview());
                        preview_m.tr_args("ui.machine.preview", &[&scheme, &host, &port])
                    })
                    .truncate()
                    .text_size(px(font::CAPTION))
                    .text_color(style::secondary_label()),
                ),
        )
        .child(
            view()
                .gap(px(style::SPACE8))
                .child(field(tr(&m, "ui.machine.field.name"), "Home", |d| d.name.clone(), |d, v| d.name = v, false))
                .child(
                    view()
                        .flex_row()
                        .gap(px(style::SPACE8))
                        .child(field(tr(&m, "ui.machine.field.host"), "controller.example.com", |d| d.host.clone(), |d, v| d.host = v, false))
                        .child(view().w(px(80.0)).child(field(tr(&m, "ui.machine.field.port"), "1-65535", |d| d.port.clone(), |d, v| d.port = v, false))),
                )
                .child(field(tr(&m, "ui.machine.field.secret"), "secret", |d| d.secret.clone(), |d, v| d.secret = v, true))
                .child(
                    view()
                        .flex_row()
                        .items_center()
                        .on_press(move || draft.update(|d| d.use_https = !d.use_https))
                        .child(text("HTTPS").flex_1().text_size(px(font::BODY)).text_color(style::primary_label()))
                        .child(crate::ui_settings::switch(move || draft.with(|d| d.use_https), || false)),
                ),
        )
        .child(view().flex_1())
        .child(
            view()
                .h(px(26.0))
                .mt(px(style::SPACE8))
                .items_center()
                .justify_center()
                .rounded(px(style::CORNER_RADIUS))
                .bg(style::info())
                .opacity(move || if https_m.machine_draft.with(MachineDraft::is_valid) { 1.0_f32 } else { 0.5_f32 })
                .on_press(save)
                .child(text(move || save_label.tr("ui.machine.save")).text_size(px(font::BODY)).font_weight(FontWeight::SEMIBOLD).text_color(style::white())),
        )
}
