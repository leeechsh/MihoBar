//! Updates of MihoBar and of its mihomo core.
//!
//! MihoBar updates itself with NSH's updater: `nsh.json`'s `"updater"`
//! names the GitHub repository and the key its `latest.json` is signed with
//! (the release workflow writes and signs it), and `prompt_for_update`
//! checks, asks in NSH's prompt (release notes, Skip This Version, Install
//! Update, automatic installs), downloads, verifies and replaces the app,
//! then relaunches it. A quiet check (at launch and daily) shows nothing
//! unless there is a version the user has not skipped.
//!
//! The core updates through mihomo's own `/upgrade` (`upgrade_core`), which
//! has no "is there a newer one?": the latest version is read from
//! MetaCubeX/mihomo's `version.txt` and compared with the running core's.
//! With automatic core updates on, a newer core installs by itself while
//! the core runs, except with TUN on, whose setuid core the upgrade would
//! replace with an unprivileged one.

use std::time::Duration;

use nsh_core::prelude::*;
use nsh_core::runtime::{UpdatePromptOptions, prompt_for_update};

use crate::model::{Model, run_root};
use crate::settings::AppLanguage;

/// Where the latest mihomo release's version is (`v1.19.34`).
const CORE_LATEST_URL: &str = "https://github.com/MetaCubeX/mihomo/releases/latest/download/version.txt";

/// Between automatic checks.
const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The first automatic check, once the core has had time to start.
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(20);

/// MihoBar's version: the packaged app's (`nsh.json` `"version"`, which a
/// release sets from its tag), else the crate's.
pub fn app_version() -> String {
    app_info().and_then(|info| info.version).unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_owned())
}

/// `v1.19.34` / `1.19.34` / `Meta v1.19.34`: its numbers, for comparing.
pub fn version_numbers(version: &str) -> Option<Vec<u64>> {
    let token = version.split_whitespace().find(|w| w.trim_start_matches('v').starts_with(|c: char| c.is_ascii_digit()))?;
    let core = token.trim_start_matches('v').split(['-', '+']).next()?;
    core.split('.').map(|p| p.parse().ok()).collect()
}

/// Whether `latest` is a newer version than `current`.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (version_numbers(latest), version_numbers(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

impl Model {
    /// The automatic checks (the menu bar process, in the installed app):
    /// 20 s after launch, then daily, each as its settings allow.
    pub fn start_update_checks(&self) {
        let m = self.clone();
        run_root(async move {
            timer(FIRST_CHECK_AFTER).await;
            loop {
                if m.settings.with_untracked(|s| s.auto_check_updates) {
                    m.check_app_update(false);
                }
                m.check_core_update(true).await;
                timer(CHECK_EVERY).await;
            }
        });
    }

    /// Checks for a newer MihoBar: `manual` (the user's "Check for
    /// Updates") reports every outcome; otherwise only a version to offer
    /// shows, and one the user chose to install automatically installs as
    /// MihoBar quits.
    pub fn check_app_update(&self, manual: bool) {
        let locale = match self.settings.with_untracked(|s| s.ui_language).resolved() {
            AppLanguage::ZhHans => "zh-Hans",
            _ => "en",
        };
        if manual {
            self.hide_panel();
        }
        spawn(prompt_for_update(UpdatePromptOptions::default().quiet(!manual).locale(locale)));
    }

    /// Reads the latest core version; `auto`: installs it when automatic
    /// core updates are on and it can (the core runs, TUN is off).
    pub async fn check_core_update(&self, auto: bool) {
        let latest = match fetch(HttpRequest::get(CORE_LATEST_URL)).await {
            Ok(response) if response.ok() => String::from_utf8_lossy(&response.body).trim().to_owned(),
            Ok(response) => {
                self.log_app("warning", self.tr_args("log.core_update.check_failed", &[&format!("HTTP {}", response.status())]));
                return;
            }
            Err(e) => {
                self.log_app("warning", self.tr_args("log.core_update.check_failed", &[&e]));
                return;
            }
        };
        if version_numbers(&latest).is_none() {
            return;
        }
        self.core_latest.set(Some(latest.clone()));
        let current = self.version.get_untracked();
        if !is_newer(&latest, &current) {
            return;
        }
        self.log_app("info", self.tr_args("log.core_update.available", &[&latest, &current]));
        let can = self.is_live_untracked() && !self.tun_enabled();
        if auto && can && self.settings.with_untracked(|s| s.auto_update_core) {
            self.upgrade_core().await;
        }
    }

    /// The running core is older than the latest release.
    pub fn core_update_available(&self) -> bool {
        let current = self.version.get();
        self.core_latest.with(|l| l.as_deref().is_some_and(|l| is_newer(l, &current)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert_eq!(version_numbers("v1.19.34"), Some(vec![1, 19, 34]));
        assert_eq!(version_numbers("Meta v1.19.34"), Some(vec![1, 19, 34]));
        assert_eq!(version_numbers("1.19.4-alpha"), Some(vec![1, 19, 4]));
        assert_eq!(version_numbers("--"), None);
        assert!(is_newer("v1.19.35", "v1.19.34"));
        assert!(is_newer("v1.20.0", "1.19.99"));
        assert!(!is_newer("v1.19.34", "v1.19.34"));
        assert!(!is_newer("v1.19.34", "--"));
    }
}
