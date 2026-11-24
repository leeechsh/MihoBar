//! Editing the core's settings and maintenance (ClashBar's
//! `AppViewModel+Settings`, `+SettingsPatch`, `+Maintenance`).

use std::time::Duration;

use nsh_core::prelude::*;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::api::{self, Endpoint};
use crate::model::{Model, run_root};
use crate::settings::CoreSettings;

/// The core's config as `/configs` reports it (ClashBar's `ConfigSnapshot`).
#[derive(Clone, Debug, PartialEq, Default, Deserialize)]
#[serde(default)]
pub struct RuntimeConfig {
    #[serde(rename = "allow-lan")]
    pub allow_lan: Option<bool>,
    pub mode: Option<String>,
    #[serde(rename = "log-level")]
    pub log_level: Option<String>,
    pub ipv6: Option<bool>,
    #[serde(rename = "tcp-concurrent")]
    pub tcp_concurrent: Option<bool>,
    pub port: Option<i64>,
    #[serde(rename = "socks-port")]
    pub socks_port: Option<i64>,
    #[serde(rename = "redir-port")]
    pub redir_port: Option<i64>,
    #[serde(rename = "tproxy-port")]
    pub tproxy_port: Option<i64>,
    #[serde(rename = "mixed-port")]
    pub mixed_port: Option<i64>,
    pub tun: Option<Value>,
}

impl RuntimeConfig {
    pub fn to_core_settings(&self) -> CoreSettings {
        let port = |v: Option<i64>| v.map(|p| p.to_string()).unwrap_or_default();
        CoreSettings {
            mode: self.mode.clone().unwrap_or_else(|| "rule".into()).to_lowercase(),
            allow_lan: self.allow_lan.unwrap_or(false),
            ipv6: self.ipv6.unwrap_or(false),
            tcp_concurrent: self.tcp_concurrent.unwrap_or(false),
            tun_enabled: self.tun.as_ref().and_then(|t| t.get("enable")).and_then(Value::as_bool).unwrap_or(false),
            log_level: self.log_level.clone().unwrap_or_else(|| "info".into()).to_lowercase(),
            port: port(self.port),
            socks_port: port(self.socks_port),
            mixed_port: port(self.mixed_port),
            redir_port: port(self.redir_port),
            tproxy_port: port(self.tproxy_port),
        }
    }
}

/// A port field's config key, label and icon.
pub const PORT_FIELDS: [(&str, &str); 5] = [
    ("port", "ui.settings.port.port"),
    ("socks-port", "ui.settings.port.socks"),
    ("mixed-port", "ui.settings.port.mixed"),
    ("redir-port", "ui.settings.port.redir"),
    ("tproxy-port", "ui.settings.port.tproxy"),
];

pub fn port_value<'a>(s: &'a CoreSettings, key: &str) -> &'a str {
    match key {
        "port" => &s.port,
        "socks-port" => &s.socks_port,
        "mixed-port" => &s.mixed_port,
        "redir-port" => &s.redir_port,
        _ => &s.tproxy_port,
    }
}

pub fn set_port_value(s: &mut CoreSettings, key: &str, value: String) {
    match key {
        "port" => s.port = value,
        "socks-port" => s.socks_port = value,
        "mixed-port" => s.mixed_port = value,
        "redir-port" => s.redir_port = value,
        _ => s.tproxy_port = value,
    }
}

/// The ports as a patch: numbers 0–65535 (an empty field is 0, or left
/// out when `skip_empty`); `Err(key)` names the first invalid one.
pub fn port_patch(s: &CoreSettings, skip_empty: bool) -> Result<Map<String, Value>, String> {
    let mut body = Map::new();
    for (key, _) in PORT_FIELDS {
        let raw = port_value(s, key).trim();
        if raw.is_empty() {
            if !skip_empty {
                body.insert(key.into(), json!(0));
            }
            continue;
        }
        match raw.parse::<i64>() {
            Ok(v) if (0..=65535).contains(&v) => {
                body.insert(key.into(), json!(v));
            }
            _ => return Err(key.to_owned()),
        }
    }
    Ok(body)
}

/// A feedback message over the settings (saved, or an error).
#[derive(Clone, Debug, PartialEq)]
pub enum Feedback {
    Saved(String),
    Error(String),
}

/// The state of a maintenance action (ClashBar's `CoreUpgradeState`,
/// `GeoUpdateState`).
#[derive(Clone, Debug, PartialEq, Default)]
pub enum TaskState {
    #[default]
    Idle,
    Running,
    Succeeded,
    AlreadyLatest(Option<String>),
    Failed(String),
}

impl Model {
    /// The settings shown: the kept ones, else the core's.
    pub fn core_settings(&self) -> CoreSettings {
        if self.is_remote() {
            return self.remote_core.get().unwrap_or_else(|| self.runtime_config.with(|r| r.to_core_settings()));
        }
        self.settings
            .with(|s| s.core_settings.clone())
            .unwrap_or_else(|| self.runtime_config.with(|r| r.to_core_settings()))
    }

    fn update_core_settings(&self, f: impl FnOnce(&mut CoreSettings)) {
        let mut next = self.core_settings();
        f(&mut next);
        if self.is_remote_untracked() {
            self.remote_core.set(Some(next));
            return;
        }
        self.settings.update(|s| s.core_settings = Some(next));
    }

    /// Reads the core's config: the kept settings start from it the first
    /// time, and follow what changed in it since it was last read
    /// (ClashBar's `syncEditableSettings`).
    pub(crate) fn sync_runtime_config(&self, incoming: RuntimeConfig) {
        let previous = self.runtime_config.get_untracked();
        let had_previous = previous != RuntimeConfig::default();
        if self.is_remote_untracked() {
            // A remote machine's settings are its own (ClashBar keeps none).
            self.remote_core.set(Some(incoming.to_core_settings()));
        } else if self.settings.with_untracked(|s| s.core_settings.is_none()) {
            self.settings.update(|s| s.core_settings = Some(incoming.to_core_settings()));
        } else if had_previous && !self.preserve_settings_on_sync.get_untracked() {
            let (old, new) = (previous.to_core_settings(), incoming.to_core_settings());
            if old != new {
                self.update_core_settings(|s| {
                    macro_rules! follow {
                        ($($f:ident),*) => { $( if old.$f != new.$f { s.$f = new.$f.clone(); } )* };
                    }
                    follow!(mode, allow_lan, ipv6, tcp_concurrent, tun_enabled, log_level, port, socks_port, mixed_port, redir_port, tproxy_port);
                });
            }
        }
        self.preserve_settings_on_sync.set(false);
        if self.runtime_config.get_untracked() != incoming {
            self.runtime_config.set(incoming);
        }
    }

    pub(crate) fn feedback_saved(&self, message: String) {
        self.show_feedback(Feedback::Saved(message));
    }

    pub(crate) fn feedback_error(&self, message: String) {
        self.show_feedback(Feedback::Error(message));
    }

    fn show_feedback(&self, feedback: Feedback) {
        self.settings_feedback.set(Some(feedback.clone()));
        if let Feedback::Saved(_) = feedback {
            let m = self.clone();
            run_root(async move {
                timer(Duration::from_secs(2)).await;
                if m.settings_feedback.get_untracked() == Some(feedback) {
                    m.settings_feedback.set(None);
                }
            });
        }
    }

    /// `PATCH /configs` with `body` (ClashBar's `patchConfigBody`).
    pub async fn patch_config(&self, body: Map<String, Value>, syncing_key: &str, success: String) -> bool {
        self.settings_syncing.set(Some(syncing_key.to_owned()));
        self.settings_feedback.set(None);
        let mut keys: Vec<&String> = body.keys().collect();
        keys.sort();
        let keys = keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ");
        self.log_app("info", format!("PATCH /configs [{keys}]"));
        let target = self.target.get_untracked();
        let result = api::request_no_response(&target, &Endpoint::PatchConfigs(Value::Object(body))).await;
        let ok = match result {
            Ok(()) => {
                self.log_app("info", format!("PATCH /configs succeeded [{keys}]"));
                if !success.is_empty() {
                    self.show_feedback(Feedback::Saved(success));
                }
                true
            }
            Err(e) => {
                self.log_app("error", format!("PATCH /configs failed [{keys}]: {e}"));
                let message = self.tr_args("app.settings.error.save_failed", &[&syncing_key, &e]);
                if syncing_key.ends_with("-overlay") {
                    self.log_app("error", message);
                } else {
                    self.show_feedback(Feedback::Error(message));
                }
                false
            }
        };
        self.refresh_from_api().await;
        self.settings_syncing.set(None);
        if ok && keys.split(", ").any(|k| matches!(k, "mixed-port" | "port" | "socks-port")) && !syncing_key.ends_with("-overlay") && !self.is_remote_untracked() {
            self.sync_system_proxy_port().await;
        }
        ok
    }

    /// Sets one boolean core setting (ClashBar's `applyEditableCoreSetting`).
    pub async fn set_core_bool(&self, key: &'static str, value: bool) {
        self.update_core_settings(|s| match key {
            "allow-lan" => s.allow_lan = value,
            "ipv6" => s.ipv6 = value,
            _ => s.tcp_concurrent = value,
        });
        if !self.is_live_untracked() {
            return;
        }
        let mut body = Map::new();
        body.insert(key.into(), json!(value));
        self.patch_config(body, key, self.tr_args("app.settings.saved.single_key", &[&key])).await;
    }

    /// Sets the core's log level.
    pub async fn set_core_log_level(&self, level: &str) {
        let level = level.trim().to_lowercase();
        if !crate::data::LOG_LEVELS.contains(&level.as_str()) {
            self.show_feedback(Feedback::Error(self.tr_args("app.settings.error.invalid_log_level", &[&level])));
            return;
        }
        self.update_core_settings(|s| s.log_level = level.clone());
        if !self.is_live_untracked() {
            return;
        }
        let mut body = Map::new();
        body.insert("log-level".into(), json!(level));
        self.patch_config(body, "log-level", self.tr_args("app.settings.saved.single_key", &[&"log-level"])).await;
    }

    /// A port field changed: kept now, saved 750 ms after the last change
    /// (ClashBar's `scheduleProxyPortsAutoSaveIfNeeded`).
    pub fn edit_port(&self, key: &str, value: String) {
        self.update_core_settings(|s| set_port_value(s, key, value));
        let generation = self.port_edit_generation.get_untracked() + 1;
        self.port_edit_generation.set(generation);
        let m = self.clone();
        run_root(async move {
            timer(Duration::from_millis(750)).await;
            if m.port_edit_generation.get_untracked() == generation && m.is_running_untracked() {
                m.apply_proxy_ports(true).await;
            }
        });
    }

    /// Saves the ports (ClashBar's `applyProxyPorts`).
    pub async fn apply_proxy_ports(&self, auto_saved: bool) {
        match port_patch(&self.core_settings(), false) {
            Ok(body) => {
                let (key, message) = if auto_saved {
                    ("ports-auto", self.tr_args("app.settings.saved.ports_auto", &[]))
                } else {
                    ("ports", self.tr_args("app.settings.saved.ports", &[]))
                };
                self.patch_config(body, key, message).await;
            }
            Err(key) => self.show_feedback(Feedback::Error(self.tr_args("app.settings.error.port_range", &[&key]))),
        }
    }

    /// Lays the kept settings over the core's config after it starts
    /// (ClashBar's `applyEditableSettingsOverlay`). TUN comes with the
    /// system proxy milestone.
    pub(crate) async fn apply_settings_overlay(&self, syncing_key: &str) {
        if self.is_remote_untracked() {
            return;
        }
        let Some(overlay) = self.settings.with_untracked(|s| s.core_settings.clone()) else { return };
        let fallback = self.runtime_config.get_untracked().to_core_settings();
        let level = if overlay.log_level.trim().is_empty() { fallback.log_level.clone() } else { overlay.log_level.clone() };
        if !crate::data::LOG_LEVELS.contains(&level.as_str()) {
            self.show_feedback(Feedback::Error(self.tr_args("app.settings.error.overlay_invalid_log_level", &[&level])));
            return;
        }
        let mut ports = overlay.clone();
        for (key, _) in PORT_FIELDS {
            if port_value(&ports, key).trim().is_empty() {
                set_port_value(&mut ports, key, port_value(&fallback, key).to_owned());
            }
        }
        let mut body = match port_patch(&ports, true) {
            Ok(body) => body,
            Err(key) => {
                self.show_feedback(Feedback::Error(self.tr_args("app.settings.error.overlay_port_range", &[&key])));
                return;
            }
        };
        let mode = if overlay.mode.is_empty() { fallback.mode.clone() } else { overlay.mode.clone() };
        body.insert("mode".into(), json!(mode));
        body.insert("allow-lan".into(), json!(overlay.allow_lan));
        body.insert("ipv6".into(), json!(overlay.ipv6));
        body.insert("tcp-concurrent".into(), json!(overlay.tcp_concurrent));
        body.insert("log-level".into(), json!(level));
        // TUN as kept, when the core may run it (ClashBar's `tunOverlayPatchBody`).
        let tun = overlay.tun_enabled && self.tun_permissions_ok();
        let mut tun_body = Map::new();
        tun_body.insert("enable".into(), json!(tun));
        if tun {
            if let Some(stack) = self.preferred_tun_stack() {
                tun_body.insert("stack".into(), json!(stack));
            }
        }
        body.insert("tun".into(), Value::Object(tun_body));
        self.patch_config(body, syncing_key, String::new()).await;
    }

    // --- Maintenance ---------------------------------------------------------

    pub async fn flush_fakeip_cache(&self) {
        self.run_named_action("log.action_name.flush_fakeip_cache", Endpoint::FlushFakeIpCache).await;
    }

    pub async fn flush_dns_cache(&self) {
        self.run_named_action("log.action_name.flush_dns_cache", Endpoint::FlushDnsCache).await;
    }

    async fn run_named_action(&self, key: &str, endpoint: Endpoint) {
        let action = self.tr_args(key, &[]);
        let target = self.target.get_untracked();
        match api::request_no_response(&target, &endpoint).await {
            Ok(()) => self.log_app("info", self.tr_args("log.action.success", &[&action])),
            Err(e) => self.log_app("error", self.tr_args("log.action.failed", &[&action, &e])),
        }
    }

    /// Updates the GEO databases (ClashBar's `upgradeGeo`).
    pub async fn upgrade_geo(&self) {
        if self.geo_update.get_untracked() == TaskState::Running {
            return;
        }
        self.geo_update.set(TaskState::Running);
        let target = self.target.get_untracked();
        let state = match api::request_no_response(&target, &Endpoint::UpgradeGeo).await {
            Ok(()) => {
                self.log_app("info", self.tr_args("log.geo_update.updated", &[]));
                TaskState::Succeeded
            }
            Err(e) => {
                let message = match &e {
                    api::ApiError::Status(_, body) if !body.trim().is_empty() => body.trim().to_owned(),
                    other => other.to_string(),
                };
                self.log_app("error", self.tr_args("log.geo_update.failed", &[&message]));
                TaskState::Failed(message)
            }
        };
        self.finish_task(self.geo_update, state);
    }

    /// Upgrades the core (ClashBar's `upgradeCore`).
    pub async fn upgrade_core(&self) {
        if self.core_upgrade.get_untracked() == TaskState::Running {
            return;
        }
        self.core_upgrade.set(TaskState::Running);
        let target = self.target.get_untracked();
        let from_message = |m: &Model, message: &str| {
            let t = message.trim();
            if t.is_empty() {
                return TaskState::Failed(m.tr_args("ui.common.unknown", &[]));
            }
            if t.to_lowercase().contains("already using latest version") {
                // The last `v1.2.3`-like token.
                let version = t
                    .split(|c: char| !(c.is_ascii_alphanumeric() || c == '.'))
                    .filter(|w| {
                        let w = w.trim_start_matches('v');
                        w.contains('.') && w.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
                    })
                    .next_back()
                    .map(str::to_owned);
                return TaskState::AlreadyLatest(version);
            }
            TaskState::Failed(t.to_owned())
        };
        #[derive(Deserialize)]
        struct Upgrade {
            status: Option<String>,
            message: Option<String>,
        }
        let state = match api::request_raw(&target, &Endpoint::UpgradeCore).await {
            Ok(body) => match serde_json::from_slice::<Upgrade>(&body) {
                Ok(u) if u.status.as_deref().is_some_and(|s| s.trim().eq_ignore_ascii_case("ok")) => TaskState::Succeeded,
                Ok(u) => from_message(self, u.message.as_deref().unwrap_or("")),
                Err(_) => from_message(self, &String::from_utf8_lossy(&body)),
            },
            Err(api::ApiError::Status(_, body)) => match serde_json::from_str::<Upgrade>(&body) {
                Ok(u) => from_message(self, u.message.as_deref().unwrap_or(&body)),
                Err(_) => from_message(self, &body),
            },
            Err(e) => from_message(self, &e.to_string()),
        };
        match &state {
            TaskState::Succeeded => {
                self.log_app("info", self.tr_args("log.core_upgrade.updated", &[]));
                let m = self.clone();
                run_root(async move {
                    timer(Duration::from_millis(750)).await;
                    m.refresh_from_api().await;
                });
            }
            TaskState::AlreadyLatest(Some(v)) => self.log_app("info", self.tr_args("log.core_upgrade.latest_version", &[v])),
            TaskState::AlreadyLatest(None) => self.log_app("info", self.tr_args("log.core_upgrade.latest", &[])),
            TaskState::Failed(message) => self.log_app("error", self.tr_args("log.core_upgrade.failed", &[message])),
            _ => {}
        }
        self.finish_task(self.core_upgrade, state);
    }

    /// Shows `state` and clears it after 4 seconds.
    fn finish_task(&self, signal: Signal<TaskState>, state: TaskState) {
        signal.set(state);
        run_root(async move {
            timer(Duration::from_secs(4)).await;
            if signal.get_untracked() != TaskState::Running {
                signal.set(TaskState::Idle);
            }
        });
    }

    pub fn show_core_dir(&self) {
        let _ = nsh_core::element::open_path(self.core_dir());
    }

    // --- App settings ----------------------------------------------------------

    /// Turns opening at login on or off (ClashBar's `applyLaunchAtLogin`).
    pub async fn apply_launch_at_login(&self, enabled: bool) {
        match nsh_core::element::desktop::set_launch_at_login(enabled).await {
            Ok(()) => {}
            Err(e) => {
                let key = if enabled { "app.launch_at_login.error.register_failed" } else { "app.launch_at_login.error.unregister_failed" };
                let message = self.tr_args(key, &[&format!("{e:?}")]);
                self.log_app("error", self.tr_args("log.launch_at_login.toggle_failed", &[&message]));
                self.show_feedback(Feedback::Error(message));
            }
        }
        self.launch_at_login.set(nsh_core::element::desktop::launch_at_login().await);
    }

    pub async fn refresh_launch_at_login(&self) {
        self.launch_at_login.set(nsh_core::element::desktop::launch_at_login().await);
    }
}
