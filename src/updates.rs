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

/// A check's outcome, shown on its row in Settings → Updates.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum CheckState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    /// A newer version: this one.
    Available(String),
    /// The latest version, when the running one is not known (the core
    /// stopped).
    Latest(String),
    Failed,
}

/// Where the latest MihoBar release's manifest is (the one NSH's updater
/// reads; the version shown before NSH's prompt verifies and installs it).
const APP_MANIFEST_URL: &str = "https://github.com/leeechsh/MihoBar/releases/latest/download/latest.json";

/// How long "Up to date" and "Check failed" stay on the row.
const OUTCOME_SHOWN: Duration = Duration::from_secs(5);

async fn fetch_text(url: &str) -> Result<String, String> {
    let response = fetch(HttpRequest::get(url)).await?;
    if !response.ok() {
        return Err(format!("HTTP {}", response.status()));
    }
    Ok(String::from_utf8_lossy(&response.body).trim().to_owned())
}

impl Model {
    /// The automatic checks (the menu bar process, in the installed app):
    /// 20 s after launch, then daily, each as its settings allow. MihoBar's
    /// goes through NSH's quiet prompt, which shows only a version the user
    /// has not skipped and installs it as MihoBar quits once the user chose
    /// automatic updates.
    pub fn start_update_checks(&self) {
        let m = self.clone();
        run_root(async move {
            timer(FIRST_CHECK_AFTER).await;
            loop {
                if m.settings.with_untracked(|s| s.auto_check_updates) {
                    m.check_app_version(false).await;
                    run_root(prompt_for_update(UpdatePromptOptions::default().quiet(true).locale(m.update_locale())));
                }
                if m.settings.with_untracked(|s| s.core_updates()) != crate::settings::CoreUpdateMode::Manual {
                    m.check_core_update(true).await;
                }
                timer(CHECK_EVERY).await;
            }
        });
    }

    fn update_locale(&self) -> &'static str {
        match self.settings.with_untracked(|s| s.ui_language).resolved() {
            AppLanguage::ZhHans => "zh-Hans",
            _ => "en",
        }
    }

    /// Shows `state` on a row; an outcome that is not an update goes back to
    /// the version after a few seconds.
    fn settle(&self, signal: Signal<CheckState>, state: CheckState) {
        let transient = matches!(state, CheckState::UpToDate | CheckState::Failed | CheckState::Latest(_));
        signal.set(state.clone());
        if transient {
            run_root(async move {
                timer(OUTCOME_SHOWN).await;
                if signal.get_untracked() == state {
                    signal.set(CheckState::Idle);
                }
            });
        }
    }

    /// Reads the latest MihoBar's version from its release manifest;
    /// `manual` (the row's Check) shows checking, up to date and failures.
    pub async fn check_app_version(&self, manual: bool) {
        if manual {
            self.app_check.set(CheckState::Checking);
        }
        let outcome = fetch_text(APP_MANIFEST_URL).await.and_then(|body| {
            serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("version").and_then(|v| v.as_str()).map(str::to_owned))
                .ok_or_else(|| "latest.json has no version".to_owned())
        });
        match outcome {
            Ok(latest) if is_newer(&latest, &app_version()) => {
                self.log_app("info", self.tr_args("log.app_update.available", &[&latest, &app_version()]));
                self.app_check.set(CheckState::Available(latest));
            }
            Ok(_) if manual => self.settle(self.app_check, CheckState::UpToDate),
            Ok(_) => {}
            Err(e) => {
                self.log_app("warning", self.tr_args("log.app_update.check_failed", &[&e]));
                if manual {
                    self.settle(self.app_check, CheckState::Failed);
                }
            }
        }
    }

    /// Installs the newer MihoBar the row offers: NSH's prompt (its notes,
    /// Install Update), which downloads, verifies and replaces the app, then
    /// relaunches it. The panel gives way to it.
    pub fn install_app_update(&self) {
        self.hide_panel();
        crate::model::activate_process(std::process::id());
        // Under the app root: a task of the button's would be cancelled as
        // the panel (and the button) goes.
        run_root(prompt_for_update(UpdatePromptOptions::default().locale(self.update_locale())));
    }

    /// Reads the latest core version; `auto`: installs it when automatic
    /// core updates are on and it can (the core runs, TUN is off); else
    /// (the row's Check) shows checking, up to date and failures.
    pub async fn check_core_update(&self, auto: bool) {
        if !auto {
            self.core_check.set(CheckState::Checking);
        }
        let latest = match fetch_text(CORE_LATEST_URL).await {
            Ok(latest) if version_numbers(&latest).is_some() => latest,
            Ok(other) => {
                self.log_app("warning", self.tr_args("log.core_update.check_failed", &[&other]));
                if !auto {
                    self.settle(self.core_check, CheckState::Failed);
                }
                return;
            }
            Err(e) => {
                self.log_app("warning", self.tr_args("log.core_update.check_failed", &[&e]));
                if !auto {
                    self.settle(self.core_check, CheckState::Failed);
                }
                return;
            }
        };
        self.core_latest.set(Some(latest.clone()));
        let current = self.version.get_untracked();
        if version_numbers(&current).is_none() {
            if !auto {
                self.settle(self.core_check, CheckState::Latest(latest));
            }
            return;
        }
        if !is_newer(&latest, &current) {
            if !auto {
                self.settle(self.core_check, CheckState::UpToDate);
            }
            return;
        }
        self.log_app("info", self.tr_args("log.core_update.available", &[&latest, &current]));
        self.core_check.set(CheckState::Idle);
        let can = self.is_live_untracked() && !self.tun_enabled();
        if auto && can && self.settings.with_untracked(|s| s.core_updates()) == crate::settings::CoreUpdateMode::Auto {
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
