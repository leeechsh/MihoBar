//! Configs from subscription links (ClashBar's `AppViewModel+RemoteConfig`):
//! importing, refreshing one or all, auto-updating, deleting configs.

use std::collections::HashMap;
use std::time::Duration;

use nsh_core::platform::HttpRequest;
use nsh_core::prelude::*;

use crate::config;
use crate::model::{Model, run_root};
use crate::settings::RemoteSubscription;

/// The largest config downloaded (ClashBar's `maxRemoteConfigBytes`).
const MAX_REMOTE_CONFIG_BYTES: usize = 5 * 1024 * 1024;
const DEFAULT_CORE_VERSION_FOR_USER_AGENT: &str = "v1.19.27";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum RemotePhase {
    #[default]
    Idle,
    Refreshing,
    Failed,
}

/// What the import form asks (ClashBar's `RemoteConfigImportInput`).
#[derive(Clone, Debug, PartialEq)]
pub struct ImportInput {
    pub url: String,
    pub file_name: String,
    pub auto_update: bool,
    pub interval_hours: i64,
}

/// The file name a link suggests (ClashBar's `inferredRemoteConfigFileName`).
pub fn inferred_file_name(url: &str) -> String {
    let path = url.split(['?', '#']).next().unwrap_or("");
    let after_scheme = path.split_once("://").map_or(path, |(_, r)| r);
    let raw = after_scheme.split_once('/').map_or("", |(_, p)| p).rsplit('/').next().unwrap_or("").trim();
    if raw.is_empty() {
        return "remote-config.yaml".into();
    }
    match raw.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("yaml") || ext.eq_ignore_ascii_case("yml") => {
            let _ = stem;
            raw.to_owned()
        }
        Some((stem, _)) if !stem.trim().is_empty() => format!("{}.yaml", stem.trim()),
        Some(_) => "remote-config.yaml".into(),
        None => format!("{raw}.yaml"),
    }
}

pub fn is_supported_url(url: &str) -> bool {
    let lower = url.trim().to_lowercase();
    (lower.starts_with("http://") || lower.starts_with("https://")) && lower.len() > 8
}

/// `自动 2 小时 5 分后 · 上次 10:20` (ClashBar's `remoteConfigMenuStatusLine`).
pub fn status_line(m: &Model, sub: &RemoteSubscription, last_update: Option<i64>, now: i64) -> Option<String> {
    // The last check, not only the last change: an unchanged download
    // counts as a check.
    let last_update = match (last_update, sub.last_check) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    let mut parts = Vec::new();
    if let Some(next) = sub.next_update_at().filter(|_| sub.auto_update) {
        let remaining = (next - now).max(0);
        let (hours, minutes) = (remaining / 3600, (remaining / 60) % 60);
        parts.push(if hours > 0 {
            m.tr_args("fmt.remote_config.next_update_hours_minutes", &[&hours, &minutes])
        } else {
            m.tr_args("fmt.remote_config.next_update_minutes", &[&minutes.max(1)])
        });
    }
    if let Some(last) = last_update {
        let full = crate::data::date_time(std::time::UNIX_EPOCH + Duration::from_secs(last.max(0) as u64));
        let today = crate::data::date_time(std::time::SystemTime::now());
        let shown = if full[..10] == today[..10] { full[11..16].to_owned() } else { full[5..16].to_owned() };
        parts.push(m.tr_args("fmt.remote_config.last_update", &[&shown]));
    }
    (!parts.is_empty()).then(|| parts.join(" · "))
}

impl Model {
    pub fn subscription(&self, file: &str) -> Option<RemoteSubscription> {
        self.settings.with(|s| s.remote_subscriptions.get(file).cloned())
    }

    /// When the config file last changed.
    pub fn config_modified_at(&self, file: &str) -> Option<i64> {
        let meta = std::fs::metadata(self.config_dir().join(file)).ok()?;
        let at = meta.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
        Some(at.as_secs() as i64)
    }

    fn set_remote_phase(&self, file: &str, phase: RemotePhase) {
        self.remote_phases.update(|p: &mut HashMap<String, RemotePhase>| {
            p.insert(file.to_owned(), phase);
        });
    }

    /// `clash.meta/<core version>` (ClashBar's `remoteSubscriptionUserAgent`).
    async fn subscription_user_agent(&self) -> String {
        let mut version = self.version.get_untracked();
        if version.trim().is_empty() && self.is_running_untracked() {
            self.refresh_from_api().await;
            version = self.version.get_untracked();
        }
        let version = version.trim();
        format!("clash.meta/{}", if version.is_empty() || version == "-" { DEFAULT_CORE_VERSION_FOR_USER_AGENT } else { version })
    }

    async fn download(&self, url: &str) -> Result<Vec<u8>, String> {
        let agent = self.subscription_user_agent().await;
        let request = HttpRequest::get(url).header("User-Agent", agent);
        let response = fetch(request).await?;
        if !response.ok() {
            return Err(format!("HTTP {}", response.status()));
        }
        if response.body.len() > MAX_REMOTE_CONFIG_BYTES {
            return Err(format!("Remote config exceeds size limit ({MAX_REMOTE_CONFIG_BYTES} bytes)"));
        }
        Ok(response.body)
    }

    fn should_reload_after(&self, updated: &[String]) -> bool {
        self.is_running_untracked() && self.selected_config_name().is_some_and(|s| updated.contains(&s))
    }

    /// Imports a subscription (ClashBar's `importRemoteConfigFile`); the
    /// result is shown in an alert.
    pub async fn import_remote_config(&self, input: ImportInput) {
        let url = input.url.trim().to_owned();
        if !is_supported_url(&url) {
            let message = self.tr_args("log.config.remote.invalid_url", &[&url]);
            self.log_app("error", message.clone());
            self.remote_import_alert(false, message).await;
            return;
        }
        let fallback = inferred_file_name(&url);
        let Some(file) = config::normalized_config_file_name(&input.file_name, Some(&fallback)) else {
            let message = self.tr_args("log.config.import.invalid_filename", &[&input.file_name]);
            self.log_app("error", message.clone());
            self.remote_import_alert(false, message).await;
            return;
        };
        let target = self.config_dir().join(&file);
        let exists = target.exists();
        if exists && self.subscription(&file).is_none() && !self.confirm_overwrite_config(&file).await {
            self.log_app("info", self.tr_args("log.config.import.cancelled", &[&file]));
            return;
        }
        let result = match self.download(&url).await {
            Ok(data) => config::write_config_data(&data, &target),
            Err(e) => Err(e),
        };
        match result {
            Ok(_) => {
                let sub = RemoteSubscription {
                    url,
                    auto_update: input.auto_update,
                    interval_hours: input.interval_hours.max(1),
                    last_check: Some(crate::format::now_secs()),
                };
                self.settings.update(|s| {
                    s.remote_subscriptions.insert(file.clone(), sub);
                });
                self.reload_configs();
                let message = self.tr_args("log.config.import_remote.success", &[&file]);
                self.log_app("info", message.clone());
                if exists && self.should_reload_after(&[file.clone()]) {
                    self.reload_config().await;
                }
                self.remote_import_alert(true, message).await;
            }
            Err(e) => {
                let message = self.tr_args("log.config.import_remote.failed", &[&file, &e]);
                self.log_app("error", message.clone());
                self.remote_import_alert(false, message).await;
            }
        }
    }

    async fn remote_import_alert(&self, success: bool, message: String) {
        let title = self.tr_args(
            if success { "app.config.remote_import.alert.success.title" } else { "app.config.remote_import.alert.failure.title" },
            &[],
        );
        let ok = self.tr_args("ui.action.ok", &[]);
        let _ = nsh_base::alert_dialog().title(title).description(message).action("ok", ok).show().await;
    }

    pub(crate) async fn confirm_overwrite_config(&self, file: &str) -> bool {
        let answer = nsh_base::alert_dialog()
            .title(self.tr_args("app.config.import.overwrite.title", &[&file]))
            .description(self.tr_args("app.config.import.overwrite.message", &[]))
            .destructive("overwrite", self.tr_args("ui.action.overwrite", &[]))
            .cancel("cancel", self.tr_args("ui.action.cancel", &[]))
            .show()
            .await;
        format!("{answer:?}").contains("overwrite")
    }

    /// Downloads one subscription again (ClashBar's `refreshRemoteConfigFile`).
    pub async fn refresh_remote_config(&self, file: String) {
        if self.remote_phases.with_untracked(|p| p.get(&file) == Some(&RemotePhase::Refreshing)) {
            return;
        }
        let Some(sub) = self.subscription(&file) else { return };
        self.set_remote_phase(&file, RemotePhase::Refreshing);
        if !is_supported_url(&sub.url) {
            let reason = self.tr_args("log.config.remote.invalid_url", &[&sub.url]);
            self.log_app("error", self.tr_args("log.config.remote.update_item_failed", &[&file, &reason]));
            self.set_remote_phase(&file, RemotePhase::Failed);
            return;
        }
        let now = crate::format::now_secs();
        let result = match self.download(&sub.url).await {
            Ok(data) => config::write_config_data(&data, &self.config_dir().join(&file)),
            Err(e) => Err(e),
        };
        self.settings.update(|s| {
            if let Some(stored) = s.remote_subscriptions.get_mut(&file) {
                stored.last_check = Some(now);
            }
        });
        match result {
            Ok(wrote) => {
                self.set_remote_phase(&file, RemotePhase::Idle);
                if !wrote {
                    self.log_app("info", self.tr_args("log.config.remote.up_to_date", &[&file]));
                }
                if wrote {
                    self.log_app("info", self.tr_args("log.config.remote.updated", &[&file]));
                    self.reload_configs();
                    if self.should_reload_after(&[file.clone()]) {
                        self.reload_config().await;
                    }
                }
            }
            Err(e) => {
                self.log_app("error", self.tr_args("log.config.remote.update_item_failed", &[&file, &e]));
                self.set_remote_phase(&file, RemotePhase::Failed);
            }
        }
    }

    /// Downloads every subscription again (ClashBar's `updateAllRemoteConfigFiles`).
    pub async fn update_all_remote_configs(&self) {
        let files: Vec<String> = self.settings.with_untracked(|s| s.remote_subscriptions.keys().cloned().collect());
        if files.is_empty() {
            let text = self.tr_args("log.config.remote.no_sources", &[]);
            self.log_app("info", text.clone());
            self.config_feedback_show(text, false);
            return;
        }
        self.config_busy.set(Some("ui.quick.update_remote_configs"));
        let total = files.len();
        let (mut updated, mut failed) = (Vec::new(), 0);
        for file in files {
            let before = self.config_modified_at(&file);
            self.refresh_remote_config(file.clone()).await;
            match self.remote_phases.with_untracked(|p| p.get(&file).copied()) {
                Some(RemotePhase::Failed) => failed += 1,
                _ if self.config_modified_at(&file) != before => updated.push(file),
                _ => {}
            }
        }
        // Downloaded: changed or already up to date.
        let ok = total.saturating_sub(failed);
        let text = self.tr_args("log.config.remote.update_result", &[&ok, &updated.len(), &failed]);
        self.log_app("info", text.clone());
        self.config_busy.set(None);
        self.config_feedback_show(text, failed > 0);
    }

    /// Deletes a config after asking (ClashBar's `deleteConfigFile`).
    pub async fn delete_config(&self, file: String) {
        let answer = nsh_base::alert_dialog()
            .title(self.tr_args("app.config.delete.title", &[&file]))
            .description(self.tr_args("app.config.delete.message", &[]))
            .destructive("delete", self.tr_args("ui.action.delete", &[]))
            .cancel("cancel", self.tr_args("ui.action.cancel", &[]))
            .show()
            .await;
        if !format!("{answer:?}").contains("delete") {
            return;
        }
        let path = self.config_dir().join(&file);
        if let Err(e) = std::fs::remove_file(&path) {
            self.log_app("error", self.tr_args("log.config.delete.failed", &[&file, &e]));
            return;
        }
        self.settings.update(|s| {
            s.remote_subscriptions.remove(&file);
        });
        self.reload_configs();
        self.log_app("info", self.tr_args("log.config.delete.success", &[&file]));
        if self.selected_config_name().as_deref() != Some(file.as_str()) {
            return;
        }
        match self.configs.get_untracked().first().cloned() {
            Some(next) => {
                self.settings.update(|s| s.selected_config = Some(next.clone()));
                self.log_app("info", self.tr_args("log.config.selected", &[&next]));
                if self.is_running_untracked() {
                    self.restart_core().await;
                }
            }
            None => {
                self.settings.update(|s| s.selected_config = None);
                if self.is_running_untracked() {
                    self.stop_core().await;
                }
            }
        }
    }

    pub fn copy_subscription_url(&self, file: &str) {
        if let Some(sub) = self.subscription(file) {
            set_clipboard(sub.url);
        }
    }

    /// Checks every minute for subscriptions due to update (ClashBar's
    /// `startRemoteConfigAutoUpdateTaskIfNeeded`).
    pub(crate) fn start_remote_auto_update(&self) {
        let m = self.clone();
        run_root(async move {
            loop {
                let now = crate::format::now_secs();
                let due: Vec<String> = m.settings.with_untracked(|s| {
                    s.remote_subscriptions.iter().filter(|(_, sub)| sub.is_due(now)).map(|(f, _)| f.clone()).collect()
                });
                for file in due {
                    m.refresh_remote_config(file).await;
                }
                timer(Duration::from_secs(60)).await;
                // The menu's "next update" lines follow the minutes.
                m.remote_clock.set(crate::format::now_secs());
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_and_due() {
        assert_eq!(inferred_file_name("https://a.com/sub/my.yml?token=1"), "my.yml");
        assert_eq!(inferred_file_name("https://a.com/api/v1/client/subscribe"), "subscribe.yaml");
        assert_eq!(inferred_file_name("https://a.com/x.txt"), "x.yaml");
        assert_eq!(inferred_file_name("https://a.com/"), "remote-config.yaml");
        assert!(is_supported_url("https://a.com/x"));
        assert!(!is_supported_url("ftp://a.com/x"));
        let sub = RemoteSubscription { url: "u".into(), auto_update: true, interval_hours: 6, last_check: Some(0) };
        assert!(!sub.is_due(3600));
        assert!(sub.is_due(6 * 3600));
        assert!(RemoteSubscription { last_check: None, ..sub.clone() }.is_due(0));
        assert!(!RemoteSubscription { auto_update: false, ..sub }.is_due(1 << 40));
    }
}
