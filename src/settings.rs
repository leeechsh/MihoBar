//! What the app remembers between launches (ClashBar keeps these in
//! UserDefaults): a JSON file in the state directory.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StatusBarDisplayMode {
    IconOnly,
    #[default]
    IconAndSpeed,
    SpeedOnly,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AppLanguage {
    /// The system's: Chinese when the user prefers Chinese before English,
    /// else English.
    #[default]
    #[serde(rename = "system")]
    System,
    #[serde(rename = "zh-Hans")]
    ZhHans,
    #[serde(rename = "en")]
    En,
}

impl AppLanguage {
    /// The language the interface shows: the system's preferred languages
    /// (NSH's `locale_settings`, tracked where it is read, so the interface
    /// follows a change) decide for `System`.
    pub fn resolved(self) -> AppLanguage {
        match self {
            AppLanguage::System => nsh_core::prelude::locale_settings()
                .languages
                .iter()
                .find_map(|l| match l.language() {
                    "zh" => Some(AppLanguage::ZhHans),
                    "en" => Some(AppLanguage::En),
                    _ => None,
                })
                .unwrap_or(AppLanguage::En),
            other => other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum AppearanceMode {
    #[default]
    System,
    Light,
    Dark,
}

/// A config imported from a subscription link (ClashBar's
/// `RemoteConfigSubscription`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RemoteSubscription {
    pub url: String,
    pub auto_update: bool,
    pub interval_hours: i64,
    /// Seconds since the epoch of the last check.
    pub last_check: Option<i64>,
}

impl RemoteSubscription {
    pub const DEFAULT_INTERVAL_HOURS: i64 = 6;

    pub fn next_update_at(&self) -> Option<i64> {
        (self.auto_update).then_some(self.last_check?).map(|l| l + self.interval_hours.max(1) * 3600)
    }

    pub fn is_due(&self, now: i64) -> bool {
        self.auto_update && self.next_update_at().is_none_or(|next| now >= next)
    }
}

/// A Wi-Fi bound to a config (ClashBar's `SSIDStrategyRule`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SsidRule {
    pub ssid: String,
    pub config: String,
}

/// The rules trimmed, empty ones dropped, one per Wi-Fi (the last wins,
/// in the place of the first; ClashBar's `SSIDStrategyRule.normalized`).
pub fn normalized_ssid_rules(rules: &[SsidRule]) -> Vec<SsidRule> {
    let mut out: Vec<SsidRule> = Vec::new();
    for r in rules {
        let (ssid, config) = (r.ssid.trim().to_owned(), r.config.trim().to_owned());
        if ssid.is_empty() || config.is_empty() {
            continue;
        }
        let rule = SsidRule { ssid, config };
        match out.iter_mut().find(|e| e.ssid == rule.ssid) {
            Some(existing) => *existing = rule,
            None => out.push(rule),
        }
    }
    out
}

/// The core settings the user edits, kept by the app and laid over the
/// core's config after it starts (ClashBar's `EditableSettingsSnapshot`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CoreSettings {
    pub mode: String,
    pub allow_lan: bool,
    pub ipv6: bool,
    pub tcp_concurrent: bool,
    pub tun_enabled: bool,
    pub log_level: String,
    pub port: String,
    pub socks_port: String,
    pub mixed_port: String,
    pub redir_port: String,
    pub tproxy_port: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The selected config's file name.
    pub selected_config: Option<String>,
    /// The config the core last started with.
    pub last_successful_config_path: Option<String>,
    pub auto_start_core: bool,
    pub status_bar_display_mode: StatusBarDisplayMode,
    pub ui_language: AppLanguage,
    pub appearance_mode: AppearanceMode,
    /// The panel stays open when it loses the focus.
    pub pinned: bool,
    /// Proxy tab: groups marked `hidden` are left out.
    pub hide_hidden_proxy_groups: bool,
    /// Proxy tab: a group's nodes by latency rather than in order.
    pub sort_nodes_by_latency: bool,
    /// Proxy tab: the last 4 delays as bars rather than the last one.
    pub show_latency_history: bool,
    /// Proxy tab: the providers section is collapsed.
    pub proxy_providers_collapsed: bool,
    /// Proxy tab: nodes without a working delay are left out.
    pub hide_unavailable_proxy_nodes: bool,
    /// Connections tab: the transport filter and the order.
    pub connections_transport_filter: crate::data::TransportFilter,
    pub connections_sort: crate::data::ConnectionSort,
    /// The core settings to lay over the config (none until first read).
    pub core_settings: Option<CoreSettings>,
    /// The system proxy was on when the app quit (it is turned on again
    /// after the next start).
    pub system_proxy_enabled_on_quit: bool,
    /// The system proxy's bypass list (none: ClashBar's defaults).
    pub system_proxy_exceptions: Option<Vec<String>>,
    /// The TUN stack chosen last.
    pub tun_stack: Option<String>,
    /// Subscriptions, by config file name.
    pub remote_subscriptions: std::collections::BTreeMap<String, RemoteSubscription>,
    /// Wi-Fi auto-switch: on, and the Wi-Fi bound to configs.
    pub ssid_strategy_enabled: bool,
    pub ssid_rules: Vec<SsidRule>,
    /// Checks for a newer MihoBar at launch and daily (NSH's update prompt).
    pub auto_check_updates: bool,
    /// Installs a newer core by itself (not with TUN on).
    pub auto_update_core: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            selected_config: None,
            last_successful_config_path: None,
            auto_start_core: false,
            status_bar_display_mode: StatusBarDisplayMode::IconAndSpeed,
            ui_language: AppLanguage::System,
            appearance_mode: AppearanceMode::System,
            pinned: false,
            hide_hidden_proxy_groups: true,
            sort_nodes_by_latency: false,
            show_latency_history: false,
            proxy_providers_collapsed: false,
            hide_unavailable_proxy_nodes: false,
            connections_transport_filter: crate::data::TransportFilter::All,
            connections_sort: crate::data::ConnectionSort::Default,
            core_settings: None,
            system_proxy_enabled_on_quit: false,
            system_proxy_exceptions: None,
            tun_stack: None,
            remote_subscriptions: Default::default(),
            ssid_strategy_enabled: false,
            ssid_rules: Vec::new(),
            auto_check_updates: true,
            auto_update_core: true,
        }
    }
}

impl Settings {
    pub fn load(path: &PathBuf) -> Self {
        let mut settings: Self = std::fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        // Every port kept as 0 (no inbound at all) was read from a core
        // that could not bind them (another client held them): laid over
        // each start, it closed the ports the config opens. They follow
        // the config again.
        if let Some(c) = settings.core_settings.as_mut() {
            let zero = |p: &str| p.trim() == "0";
            if [&c.port, &c.socks_port, &c.mixed_port, &c.redir_port, &c.tproxy_port].iter().all(|p| zero(p)) {
                for p in [&mut c.port, &mut c.socks_port, &mut c.mixed_port, &mut c.redir_port, &mut c.tproxy_port] {
                    p.clear();
                }
            }
        }
        settings
    }

    pub fn save(&self, path: &PathBuf) {
        let Ok(bytes) = serde_json::to_vec_pretty(self) else { return };
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, bytes).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}
