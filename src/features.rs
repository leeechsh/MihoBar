//! The system proxy and TUN (ClashBar's `AppViewModel+SystemProxy`,
//! `+Tun` and the core feature recovery of `+Lifecycle`).

use std::time::Duration;

use nsh_core::prelude::*;
use serde_json::{Map, Value, json};

use crate::api::{self, Endpoint};
use crate::model::Model;
use crate::sysproxy::{self, SystemProxyPorts};

/// The TUN stacks offered (ClashBar's `tunStackOptions`).
pub const TUN_STACKS: [&str; 4] = ["system", "gvisor", "mixed", "mips"];
/// The core that first has the `mips` stack.
pub const MIPS_MIN_CORE: &str = "1.19.31";

/// `v1.19.34` → (1, 19, 34).
fn version_tuple(v: &str) -> Option<(u32, u32, u32)> {
    let mut parts = v.trim().trim_start_matches('v').split(['.', '-']).map(|p| p.parse::<u32>().ok());
    Some((parts.next()??, parts.next()??, parts.next().flatten().unwrap_or(0)))
}

impl Model {
    fn sp_error(&self, e: &sysproxy::Error) -> String {
        let (key, arg) = e.message_key();
        match arg {
            Some(a) => self.tr_args(key, &[&a]),
            None => self.tr_args(key, &[]),
        }
    }

    fn sp_hint(&self, e: &sysproxy::Error) -> String {
        let key = e.message_key().0.replace(".error.", ".alert.");
        let key = match e {
            sysproxy::Error::RequiresApplications => "app.system_proxy.alert.helper_install_location".to_owned(),
            _ => key,
        };
        self.tr_args(&key, &[])
    }

    /// Where the system proxy points: this machine and the core's ports
    /// (ClashBar's `resolveSystemProxyTargetFromRuntimeConfig`).
    async fn system_proxy_target(&self) -> Result<(String, SystemProxyPorts), sysproxy::Error> {
        self.refresh_from_api().await;
        let c = self.runtime_config.get_untracked();
        let ports = SystemProxyPorts::resolve(c.mixed_port, c.port, c.socks_port);
        if !ports.has_enabled_port() {
            return Err(sysproxy::Error::InvalidPort);
        }
        let host = self.target.with_untracked(|t| t.controller.rsplit_once(':').map(|(h, _)| h.to_owned()));
        let host = host.filter(|h| !h.is_empty() && h != "0.0.0.0").unwrap_or_else(|| "127.0.0.1".into());
        Ok((host, ports))
    }

    /// Turns the system proxy on or off (ClashBar's `toggleSystemProxy`).
    pub async fn toggle_system_proxy(&self, enabled: bool) {
        if self.proxy_syncing.get_untracked() {
            return;
        }
        self.proxy_syncing.set(true);
        self.system_proxy_hint.set(None);
        let result: Result<Option<String>, sysproxy::Error> = async {
            if enabled {
                let (host, ports) = self.system_proxy_target().await?;
                let exceptions = self.saved_exceptions();
                let (h, p) = (host.clone(), ports);
                blocking(move || {
                    sysproxy::apply(true, &h, &p)?;
                    sysproxy::set_exceptions(&exceptions)
                })
                .await
                .map_err(|e| sysproxy::Error::OperationFailed(e.to_string()))??;
                Ok(sysproxy::display(&host, &ports))
            } else {
                blocking(|| sysproxy::apply(false, "", &SystemProxyPorts::default()))
                    .await
                    .map_err(|e| sysproxy::Error::OperationFailed(e.to_string()))??;
                Ok(None)
            }
        }
        .await;
        match result {
            Ok(display) => {
                self.system_proxy_enabled.set(enabled);
                self.system_proxy_display.set(display);
                let state = self.tr_args(if enabled { "log.system_proxy.enabled" } else { "log.system_proxy.disabled" }, &[]);
                self.log_app("info", self.tr_args("log.system_proxy.toggled", &[&state]));
            }
            Err(e) => {
                self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&self.sp_error(&e)]));
                if enabled {
                    self.system_proxy_hint.set(Some(self.sp_hint(&e)));
                }
                self.refresh_system_proxy_status().await;
            }
        }
        self.proxy_syncing.set(false);
    }

    /// Reads whether the system proxy is on (when the helper is there).
    pub async fn refresh_system_proxy_status(&self) {
        if let Ok(Ok((on, display))) = blocking(|| Ok::<_, sysproxy::Error>((sysproxy::is_enabled()?, sysproxy::active_display()?))).await {
            self.system_proxy_enabled.set(on);
            self.system_proxy_display.set(if on { display } else { None });
        }
    }

    /// The core's ports changed: the system proxy follows them.
    pub(crate) async fn sync_system_proxy_port(&self) {
        if !self.system_proxy_enabled.get_untracked() {
            return;
        }
        match self.system_proxy_target().await {
            Ok((host, ports)) => {
                let (h, p) = (host.clone(), ports);
                match blocking(move || sysproxy::apply(true, &h, &p)).await {
                    Ok(Ok(())) => {
                        self.system_proxy_display.set(sysproxy::display(&host, &ports));
                        self.log_app("info", self.tr_args("log.system_proxy.port_synced", &[&ports.primary().unwrap_or(0)]));
                    }
                    Ok(Err(e)) => self.log_app("error", self.tr_args("log.system_proxy.port_sync_failed", &[&self.sp_error(&e)])),
                    Err(e) => self.log_app("error", self.tr_args("log.system_proxy.port_sync_failed", &[&e])),
                }
            }
            Err(e) => self.log_app("error", self.tr_args("log.system_proxy.port_sync_failed", &[&self.sp_error(&e)])),
        }
    }

    // --- Exceptions ---------------------------------------------------------------

    fn saved_exceptions(&self) -> Vec<String> {
        self.settings
            .with_untracked(|s| s.system_proxy_exceptions.clone())
            .unwrap_or_else(|| sysproxy::DEFAULT_EXCEPTIONS.iter().map(|s| s.to_string()).collect())
    }

    fn normalized(values: &[String]) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        values.iter().map(|v| v.trim().to_owned()).filter(|v| !v.is_empty() && seen.insert(v.to_lowercase())).collect()
    }

    pub fn add_exception_draft(&self) {
        let value = self.new_exception.get_untracked().trim().to_owned();
        if value.is_empty() {
            return;
        }
        if !self.exceptions_draft.with_untracked(|d| d.iter().any(|e| e.eq_ignore_ascii_case(&value))) {
            self.exceptions_draft.update(|d| d.push(value));
        }
        self.new_exception.set(String::new());
    }

    pub fn remove_exception_draft(&self, index: usize) {
        self.exceptions_draft.update(|d| {
            if index < d.len() {
                d.remove(index);
            }
        });
    }

    pub fn restore_default_exceptions_draft(&self) {
        self.exceptions_draft.set(sysproxy::DEFAULT_EXCEPTIONS.iter().map(|s| s.to_string()).collect());
    }

    /// Saves the bypass list: to the system through the helper, then here
    /// (ClashBar's `saveSystemProxyExceptions`).
    pub async fn save_exceptions(&self) {
        let list = Self::normalized(&self.exceptions_draft.get_untracked());
        self.exceptions_draft.set(list.clone());
        self.settings_syncing.set(Some("system-proxy-exceptions".into()));
        let l = list.clone();
        let result = blocking(move || sysproxy::set_exceptions(&l)).await;
        self.settings_syncing.set(None);
        match result {
            Ok(Ok(())) => {
                self.settings.update(|s| s.system_proxy_exceptions = Some(list.clone()));
                self.exceptions_saved.set(list);
                self.feedback_saved(self.tr_args("app.settings.saved.system_proxy_exceptions", &[]));
            }
            Ok(Err(e)) => {
                let message = self.tr_args("app.settings.error.save_failed", &[&"system-proxy-exceptions", &self.sp_error(&e)]);
                self.feedback_error(message);
            }
            Err(e) => self.feedback_error(e.to_string()),
        }
    }

    // --- TUN ------------------------------------------------------------------------

    pub fn tun_enabled(&self) -> bool {
        self.runtime_config.with(|c| c.tun.as_ref().and_then(|t| t.get("enable")).and_then(Value::as_bool).unwrap_or(false))
            && self.is_live()
    }

    pub fn tun_stack(&self) -> Option<String> {
        self.runtime_config.with(|c| c.tun.as_ref().and_then(|t| t.get("stack")).and_then(Value::as_str).map(str::to_lowercase))
    }

    pub fn is_mips_supported(&self) -> bool {
        match (version_tuple(&self.version.get()), version_tuple(MIPS_MIN_CORE)) {
            (Some(a), Some(b)) => a >= b,
            _ => false,
        }
    }

    /// The managed core is root-owned and setuid (ClashBar's
    /// `hasRequiredPermissions`).
    pub fn tun_permissions_ok(&self) -> bool {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(self.core_dir().join("mihomo")).is_ok_and(|m| m.uid() == 0 && m.mode() & 0o4000 != 0 && m.mode() & 0o100 != 0)
    }

    /// Grants the core root (an administrator prompt) when it lacks it
    /// (ClashBar's `ensureTunPermissions`): `Ok(true)` when it was granted
    /// now, so a core already running is still the unprivileged process and
    /// must restart before it can open a TUN device.
    async fn ensure_tun_permissions(&self, request_if_missing: bool) -> Result<bool, String> {
        let binary = self.core_dir().join("mihomo");
        if !binary.exists() {
            return Err(self.tr_args("app.tun.error.binary_not_found", &[&self.core_dir().display()]));
        }
        if self.tun_permissions_ok() {
            return Ok(false);
        }
        if !request_if_missing {
            return Err(self.tr_args("app.tun.error.permission_missing", &[]));
        }
        self.log_app("info", self.tr_args("log.tun.permission_requesting", &[]));
        let path = sysproxy::shell_quoted(&binary.display().to_string());
        let command = format!("/usr/sbin/chown root:admin {path} && /bin/chmod u+s {path}");
        let cancelled = self.tr_args("app.tun.error.authorization_cancelled", &[]);
        let result = blocking(move || {
            sysproxy::run_as_administrator(&command, sysproxy::Error::AuthorizationCancelled, sysproxy::Error::InstallFailed)
        })
        .await
        .map_err(|e| e.to_string())?;
        match result {
            Ok(()) if self.tun_permissions_ok() => {
                self.log_app("info", self.tr_args("log.tun.permission_granted", &[]));
                Ok(true)
            }
            Ok(()) => Err(self.tr_args("app.tun.error.permission_verify_failed", &[])),
            Err(sysproxy::Error::AuthorizationCancelled) => Err(cancelled),
            Err(sysproxy::Error::InstallFailed(m)) => Err(self.tr_args("app.tun.error.authorization_failed", &[&m])),
            Err(e) => Err(self.sp_error(&e)),
        }
    }

    /// TUN on a core that just got root: kept on (with `stack`), the core
    /// restarts as the privileged process, and its start turns TUN on from
    /// the kept settings (or the request after it, should it not).
    async fn restart_for_tun(&self, stack: Option<String>) -> Result<(), String> {
        self.settings.update(|s| {
            if let Some(stack) = stack.clone() {
                s.tun_stack = Some(stack);
            }
            if let Some(c) = s.core_settings.as_mut() {
                c.tun_enabled = true;
            }
        });
        self.log_app("info", self.tr_args("log.tun.restarting_with_root", &[]));
        self.restart_core().await;
        if !self.is_live_untracked() {
            return Err(self.tr_args("app.tun.error.runtime_state_mismatch", &[]));
        }
        self.refresh_from_api().await;
        if !self.tun_enabled() {
            self.patch_tun(true, stack).await?;
            self.refresh_from_api().await;
        }
        if self.tun_enabled() { Ok(()) } else { Err(self.tr_args("app.tun.error.runtime_state_mismatch", &[])) }
    }

    pub(crate) fn preferred_tun_stack(&self) -> Option<String> {
        self.settings.with_untracked(|s| s.tun_stack.clone()).or_else(|| Some("mixed".into()))
    }

    async fn patch_tun(&self, enable: bool, stack: Option<String>) -> Result<(), String> {
        let mut tun = Map::new();
        tun.insert("enable".into(), json!(enable));
        if let Some(stack) = stack.or_else(|| enable.then(|| self.preferred_tun_stack()).flatten()) {
            tun.insert("stack".into(), json!(stack));
        }
        let target = self.target.get_untracked();
        api::request_no_response(&target, &Endpoint::PatchConfigs(json!({ "tun": Value::Object(tun) })))
            .await
            .map_err(|e| self.tr_args("app.tun.error.patch_failed", &[&e]))
    }

    /// Turns TUN on or off (ClashBar's `toggleTunMode`).
    pub async fn toggle_tun(&self, enabled: bool) {
        if self.tun_syncing.get_untracked() || enabled == self.tun_enabled() {
            return;
        }
        self.tun_syncing.set(true);
        let result: Result<(), String> = async {
            if enabled && self.ensure_tun_permissions(true).await? && self.is_live_untracked() {
                return self.restart_for_tun(None).await;
            }
            if !self.is_live_untracked() {
                return Ok(());
            }
            self.patch_tun(enabled, None).await?;
            self.refresh_from_api().await;
            if self.tun_enabled() != enabled {
                return Err(self.tr_args("app.tun.error.runtime_state_mismatch", &[]));
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                self.settings.update(|s| {
                    if let Some(c) = s.core_settings.as_mut() {
                        c.tun_enabled = enabled;
                    }
                });
                let state = self.tr_args(if enabled { "log.tun.enabled" } else { "log.tun.disabled" }, &[]);
                self.log_app("info", self.tr_args("log.tun.toggled", &[&state]));
            }
            Err(e) => {
                let message = self.tr_args("log.tun.toggle_failed", &[&e]);
                self.log_app("error", message.clone());
                self.feedback_error(message);
                self.refresh_from_api().await;
            }
        }
        self.tun_syncing.set(false);
    }

    /// Switches the TUN stack, turning TUN on (ClashBar's `selectTunStack`).
    pub async fn select_tun_stack(&self, stack: &str) {
        if self.tun_syncing.get_untracked() {
            return;
        }
        if stack.eq_ignore_ascii_case("mips") && !self.is_mips_supported() {
            self.log_app("warning", self.tr_args("app.tun.mips_requires_upgrade", &[&MIPS_MIN_CORE]));
            return;
        }
        if self.tun_enabled() && self.tun_stack().as_deref() == Some(stack) {
            return;
        }
        self.tun_syncing.set(true);
        let result: Result<(), String> = async {
            if self.ensure_tun_permissions(true).await? && self.is_live_untracked() {
                return self.restart_for_tun(Some(stack.to_owned())).await;
            }
            if !self.is_live_untracked() {
                return Ok(());
            }
            self.patch_tun(true, Some(stack.to_owned())).await?;
            self.refresh_from_api().await;
            if !self.tun_enabled() {
                return Err(self.tr_args("app.tun.error.runtime_state_mismatch", &[]));
            }
            Ok(())
        }
        .await;
        match result {
            Ok(()) => {
                let s = stack.to_owned();
                self.settings.update(|st| {
                    st.tun_stack = Some(s);
                    if let Some(c) = st.core_settings.as_mut() {
                        c.tun_enabled = true;
                    }
                });
                self.log_app("info", self.tr_args("log.tun.stack_changed", &[&stack]));
            }
            Err(e) => {
                let message = self.tr_args("log.tun.toggle_failed", &[&e]);
                self.log_app("error", message.clone());
                self.feedback_error(message);
                self.refresh_from_api().await;
            }
        }
        self.tun_syncing.set(false);
    }

    // --- Recovery around stops, restarts and quits ----------------------------------

    /// At launch: what was on at the last quit comes back after the next
    /// start; the bypass list is loaded (ClashBar's
    /// `seedCoreFeatureRecoveryFromPersistedQuitState`).
    pub(crate) fn seed_features_on_launch(&self) {
        if self.settings.with_untracked(|s| s.system_proxy_enabled_on_quit) {
            self.settings.update(|s| s.system_proxy_enabled_on_quit = false);
            self.pending_recovery.update(|r| r.0 = true);
        }
        let saved = self.saved_exceptions();
        self.exceptions_draft.set(saved.clone());
        self.exceptions_saved.set(saved);
    }

    /// Before a stop or restart: the system proxy goes off and is noted to
    /// come back (ClashBar's `prepareCoreFeatureRecoveryBeforeCoreTransition`).
    pub(crate) async fn prepare_feature_recovery(&self) {
        let running = self.is_running_untracked();
        let proxy = running && self.system_proxy_enabled.get_untracked();
        let tun = running && self.tun_enabled();
        if proxy || tun {
            self.pending_recovery.update(|r| {
                r.0 |= proxy;
                r.1 |= tun;
            });
        }
        if tun {
            let state = self.tr_args("log.tun.disabled", &[]);
            self.log_app("info", self.tr_args("log.tun.toggled", &[&state]));
        }
        if !self.system_proxy_enabled.get_untracked() {
            return;
        }
        self.proxy_syncing.set(true);
        match blocking(|| sysproxy::apply(false, "", &SystemProxyPorts::default())).await {
            Ok(Ok(())) => {
                self.system_proxy_enabled.set(false);
                self.system_proxy_display.set(None);
                let state = self.tr_args("log.system_proxy.disabled", &[]);
                self.log_app("info", self.tr_args("log.system_proxy.toggled", &[&state]));
            }
            Ok(Err(e)) => self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&self.sp_error(&e)])),
            Err(e) => self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&e])),
        }
        self.proxy_syncing.set(false);
    }

    /// After a start: turns back on what a stop turned off (ClashBar's
    /// `restoreCoreFeaturesAfterStartupIfNeeded`).
    pub(crate) async fn restore_core_features(&self) {
        let (proxy, tun) = self.pending_recovery.get_untracked();
        if !(proxy || tun) || !self.is_running_untracked() {
            return;
        }
        let mut remaining = (proxy, tun);
        if tun && self.tun_permissions_ok() {
            if !self.tun_enabled() {
                if self.patch_tun(true, None).await.is_ok() {
                    self.refresh_from_api().await;
                }
            }
            if self.tun_enabled() {
                remaining.1 = false;
                let state = self.tr_args("log.tun.enabled", &[]);
                self.log_app("info", self.tr_args("log.tun.toggled", &[&state]));
            }
        }
        if proxy {
            match self.system_proxy_target().await {
                Ok((host, ports)) => {
                    let (h, p) = (host.clone(), ports);
                    let result = blocking(move || {
                        if !sysproxy::is_configured(&h, &p).unwrap_or(false) {
                            sysproxy::apply(true, &h, &p)?;
                        }
                        Ok::<_, sysproxy::Error>(())
                    })
                    .await;
                    match result {
                        Ok(Ok(())) => {
                            remaining.0 = false;
                            self.system_proxy_enabled.set(true);
                            self.system_proxy_display.set(sysproxy::display(&host, &ports));
                            let state = self.tr_args("log.system_proxy.enabled", &[]);
                            self.log_app("info", self.tr_args("log.system_proxy.toggled", &[&state]));
                        }
                        Ok(Err(e)) => {
                            self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&self.sp_error(&e)]));
                            self.system_proxy_hint.set(Some(self.sp_hint(&e)));
                        }
                        Err(e) => self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&e])),
                    }
                }
                Err(e) => self.log_app("error", self.tr_args("log.system_proxy.toggle_failed", &[&self.sp_error(&e)])),
            }
        }
        self.pending_recovery.set(remaining);
    }

    /// At quit: notes whether the system proxy was on and turns it off,
    /// waiting at most 2 seconds (ClashBar's `shutdownForTermination`).
    pub(crate) async fn prepare_quit_features(&self) {
        let on = self.system_proxy_enabled.get_untracked();
        self.settings.update(|s| s.system_proxy_enabled_on_quit = on);
        if on {
            let _ = blocking(|| sysproxy::clear_blocking(Duration::from_secs(2))).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(version_tuple("v1.19.34") >= version_tuple(MIPS_MIN_CORE));
        assert!(version_tuple("1.19.30") < version_tuple(MIPS_MIN_CORE));
        assert_eq!(version_tuple("--"), None);
    }
}

// --- Network ---------------------------------------------------------------------

impl Model {
    /// Watches the network: the core stops while it is offline and starts
    /// again when it is back (ClashBar's `AppViewModel+NetworkMonitoring`).
    pub(crate) fn start_network_monitoring(&self) {
        // The system's route (NSH's `network()`: on macOS the path monitor,
        // ClashBar's `NetworkReachabilityMonitor`); reading it subscribes, so
        // this runs again as the route changes. Unknown (not answered yet,
        // or a platform NSH does not watch) changes nothing.
        let m = self.clone();
        effect(move || {
            let kind = network().kind;
            if kind == nsh_core::platform::NetworkKind::Unknown {
                return;
            }
            let online = kind != nsh_core::platform::NetworkKind::None;
            untracked(|| {
                let previous = m.network_online.get_untracked();
                m.network_online.set(Some(online));
                if previous == Some(online) {
                    return;
                }
                let m = m.clone();
                crate::model::run_root(async move {
                    if online {
                        m.auto_start_for_network_recovery().await;
                    } else {
                        m.auto_stop_for_network_loss().await;
                    }
                });
            });
        });
    }

    async fn auto_stop_for_network_loss(&self) {
        let mut logged = false;
        for _ in 0..120 {
            if self.network_online.get_untracked() != Some(false) || !self.is_running_untracked() {
                return;
            }
            self.resume_after_network.set(true);
            if self.action.get_untracked() != crate::model::CoreAction::Idle {
                timer(Duration::from_millis(250)).await;
                continue;
            }
            if !logged {
                logged = true;
                self.log_app("warning", self.tr_args("log.network.offline_auto_stop", &[]));
            }
            // TUN goes off first (ClashBar's `disableRuntimeTunBeforeStop`).
            if self.tun_enabled() {
                let _ = self.patch_tun(false, None).await;
            }
            self.stop_core_keeping_resume().await;
            timer(Duration::from_millis(250)).await;
        }
    }

    async fn auto_start_for_network_recovery(&self) {
        let mut logged = false;
        for _ in 0..120 {
            if self.network_online.get_untracked() != Some(true) || !self.resume_after_network.get_untracked() {
                return;
            }
            if self.action.get_untracked() != crate::model::CoreAction::Idle {
                timer(Duration::from_millis(250)).await;
                continue;
            }
            if self.is_running_untracked() {
                self.restore_core_features().await;
                self.resume_after_network.set(false);
                return;
            }
            self.resume_after_network.set(false);
            if !logged {
                logged = true;
                self.log_app("info", self.tr_args("log.network.online_auto_start", &[]));
            }
            self.start_core(crate::model::StartTrigger::Auto).await;
            if self.is_running_untracked() {
                return;
            }
            self.resume_after_network.set(true);
            timer(Duration::from_millis(500)).await;
        }
    }
}

// --- Wi-Fi auto-switch -------------------------------------------------------------

impl Model {
    /// Reads the Wi-Fi every 5 seconds while auto-switch is on and applies
    /// its rule when it changes (ClashBar's SSID strategy).
    pub(crate) fn start_ssid_monitoring(&self) {
        #[cfg(target_os = "macos")]
        {
            let m = self.clone();
            crate::model::run_root(async move {
                loop {
                    if m.settings.with_untracked(|s| s.ssid_strategy_enabled) {
                        m.refresh_ssid().await;
                    }
                    timer(Duration::from_secs(5)).await;
                }
            });
        }
    }

    /// Reads the permission and the Wi-Fi; a new Wi-Fi applies its rule.
    pub async fn refresh_ssid(&self) {
        {
            let auth: crate::wifi_types::Authorization = nsh_extra::location::authorization().await.into();
            #[cfg(target_os = "macos")]
            let ssid = if auth == crate::wifi_types::Authorization::Authorized {
                run_on_main_thread(crate::wifi::current_ssid).await.ok().flatten()
            } else {
                None
            };
            #[cfg(not(target_os = "macos"))]
            let ssid: Option<String> = None;
            if self.ssid_authorization.get_untracked() != auth {
                self.ssid_authorization.set(auth);
            }
            let previous = self.current_ssid.get_untracked();
            if previous != ssid {
                self.current_ssid.set(ssid);
                self.apply_ssid_strategy().await;
            }
        }
    }

    /// Turns auto-switch on or off; on asks for the location permission.
    pub async fn set_ssid_strategy_enabled(&self, enabled: bool) {
        self.settings.update(|s| s.ssid_strategy_enabled = enabled);
        if enabled {
            let _ = nsh_extra::location::request_authorization().await;
            self.refresh_ssid().await;
            if self.ssid_authorization.get_untracked() != crate::wifi_types::Authorization::Authorized {
                self.log_app("warning", self.tr_args("log.ssid_strategy.authorization_required", &[]));
            }
            self.apply_ssid_strategy().await;
        }
    }

    pub async fn request_ssid_permission(&self) {
        let _ = nsh_extra::location::request_authorization().await;
        self.refresh_ssid().await;
    }

    /// The Wi-Fi bound to `config`.
    pub fn ssids_for_config(&self, config: &str) -> Vec<String> {
        self.settings.with(|s| s.ssid_rules.iter().filter(|r| r.config == config).map(|r| r.ssid.clone()).collect())
    }

    /// Binds the current Wi-Fi to `config`, or unbinds it when it already
    /// is (ClashBar's `toggleCurrentSSIDBinding`).
    pub async fn toggle_current_ssid_binding(&self, config: String) {
        if !self.settings.with_untracked(|s| s.ssid_strategy_enabled) {
            self.log_app("warning", self.tr_args("log.ssid_strategy.enable_auto_switch_first", &[]));
            return;
        }
        if self.ssid_authorization.get_untracked() != crate::wifi_types::Authorization::Authorized {
            self.log_app("warning", self.tr_args("log.ssid_strategy.authorization_required", &[]));
            return;
        }
        let Some(ssid) = self.current_ssid.get_untracked() else {
            self.log_app("warning", self.tr_args("log.ssid_strategy.current_wifi_unavailable", &[]));
            return;
        };
        let bound = self.settings.with_untracked(|s| s.ssid_rules.iter().any(|r| r.ssid == ssid && r.config == config));
        if bound {
            self.remove_ssid_binding(&ssid);
        } else {
            self.settings.update(|s| {
                let mut rules = s.ssid_rules.clone();
                rules.push(crate::settings::SsidRule { ssid: ssid.clone(), config: config.clone() });
                s.ssid_rules = crate::settings::normalized_ssid_rules(&rules);
            });
            self.log_app("info", self.tr_args("log.ssid_strategy.bound", &[&ssid, &config]));
            self.apply_ssid_strategy().await;
        }
    }

    pub fn remove_ssid_binding(&self, ssid: &str) {
        let ssid = ssid.trim().to_owned();
        self.settings.update(|s| s.ssid_rules.retain(|r| r.ssid != ssid));
        self.log_app("info", self.tr_args("log.ssid_strategy.unbound", &[&ssid]));
    }

    /// Switches to the config the current Wi-Fi is bound to (ClashBar's
    /// `applySSIDStrategyForCurrentSSIDIfNeeded`).
    pub async fn apply_ssid_strategy(&self) {
        if !self.settings.with_untracked(|s| s.ssid_strategy_enabled) {
            return;
        }
        let Some(ssid) = self.current_ssid.get_untracked() else { return };
        let Some(rule) = self.settings.with_untracked(|s| s.ssid_rules.iter().find(|r| r.ssid == ssid).cloned()) else {
            return;
        };
        if !self.configs.get_untracked().contains(&rule.config) {
            self.log_app("warning", self.tr_args("log.ssid_strategy.config_missing", &[&rule.config]));
            return;
        }
        if self.selected_config_name().as_deref() == Some(rule.config.as_str()) {
            return;
        }
        self.log_app("info", self.tr_args("log.ssid_strategy.switching", &[&ssid, &rule.config]));
        self.select_config(rule.config).await;
    }
}
