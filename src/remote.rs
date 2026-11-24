//! Remote machines (ClashBar's `RemoteMachine`, `RemoteMachineStore` and
//! `AppViewModel+RemoteMachine`): other mihomo controllers the panel can
//! show and drive instead of the local core.

use std::collections::HashMap;
use std::time::Duration;

use futures_util::future::{Either, join_all, select};
use nsh_core::platform::HttpRequest;
use nsh_core::prelude::*;
use serde::{Deserialize, Serialize};

use crate::api::ApiTarget;
use crate::model::{ApiStatus, Model, RuntimeStatus, run_root};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RemoteMachine {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub use_https: bool,
}

impl RemoteMachine {
    pub fn new_id() -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64);
        format!("{nanos:x}-{:x}", COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    pub fn controller_address(&self) -> String {
        if self.use_https {
            format!("https://{}:{}", self.host, self.port)
        } else {
            self.display_address()
        }
    }

    pub fn display_address(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    pub fn api_target(&self) -> ApiTarget {
        ApiTarget { controller: self.controller_address(), secret: self.secret.clone().filter(|s| !s.is_empty()) }
    }
}

/// ClashBar's `MachineConnectionStatus`.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum MachineStatus {
    #[default]
    Unknown,
    Checking,
    Connected(String),
    Failed(String),
}

impl MachineStatus {
    pub fn is_connected(&self) -> bool {
        matches!(self, Self::Connected(_))
    }
}

/// The editor's fields (ClashBar's `RemoteMachineEditorView`).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MachineDraft {
    pub name: String,
    pub host: String,
    pub port: String,
    pub secret: String,
    pub use_https: bool,
}

impl MachineDraft {
    pub fn from_machine(m: Option<&RemoteMachine>) -> Self {
        match m {
            Some(m) => Self {
                name: m.name.clone(),
                host: m.host.clone(),
                port: m.port.to_string(),
                secret: m.secret.clone().unwrap_or_default(),
                use_https: m.use_https,
            },
            None => Self { port: "9090".into(), ..Default::default() },
        }
    }

    fn port_value(&self) -> Option<u16> {
        self.port.trim().parse::<u16>().ok().filter(|p| *p > 0)
    }

    pub fn is_valid(&self) -> bool {
        !self.name.trim().is_empty() && !self.host.trim().is_empty() && self.port_value().is_some()
    }

    /// ClashBar's `connectionPreview` arguments: scheme, host, port.
    pub fn preview(&self) -> (String, String, String) {
        let host = if self.host.trim().is_empty() { "controller.example.com".into() } else { self.host.clone() };
        let port = if self.port.trim().is_empty() { "9090".into() } else { self.port.clone() };
        ((if self.use_https { "https" } else { "http" }).into(), host, port)
    }

    pub fn to_machine(&self, id: Option<String>) -> RemoteMachine {
        let secret = self.secret.trim().to_owned();
        RemoteMachine {
            id: id.unwrap_or_else(RemoteMachine::new_id),
            name: self.name.trim().to_owned(),
            host: self.host.trim().to_owned(),
            port: self.port_value().unwrap_or(9090),
            secret: (!secret.is_empty()).then_some(secret),
            use_https: self.use_https,
        }
    }
}

/// Reads `/version` with a one-second limit (ClashBar's `probe`).
pub async fn probe(machine: &RemoteMachine) -> MachineStatus {
    let url = format!("{}/version", machine.api_target().base_url());
    let mut request = HttpRequest::new("GET", url);
    if let Some(secret) = machine.secret.as_ref().filter(|s| !s.is_empty()) {
        request = request.header("Authorization", format!("Bearer {secret}"));
    }
    let response = match select(Box::pin(fetch(request)), Box::pin(timer(Duration::from_secs(1)))).await {
        Either::Left((r, _)) => r,
        Either::Right(_) => return MachineStatus::Failed("Timeout".into()),
    };
    let response = match response {
        Ok(r) => r,
        Err(e) => {
            let lower = e.to_lowercase();
            let reason = if lower.contains("refused") { "Connection refused".to_owned() } else { e };
            return MachineStatus::Failed(reason);
        }
    };
    match response.status() {
        200 => {}
        401 => return MachineStatus::Failed("Auth failed (401)".into()),
        code => return MachineStatus::Failed(format!("HTTP {code}")),
    }
    let version = serde_json::from_slice::<HashMap<String, serde_json::Value>>(&response.body)
        .ok()
        .and_then(|m| m.get("version").and_then(|v| v.as_str()).map(str::to_owned));
    MachineStatus::Connected(version.unwrap_or_else(|| "OK".into()))
}

impl Model {
    pub fn is_remote(&self) -> bool {
        self.remote.with(Option::is_some)
    }

    pub fn is_remote_untracked(&self) -> bool {
        self.remote.with_untracked(Option::is_some)
    }

    /// A controller is there to talk to: a remote machine, or the local
    /// core running (ClashBar's `isRemoteTarget || isRuntimeRunning`).
    pub fn is_live(&self) -> bool {
        self.is_remote() || self.status.get() == RuntimeStatus::Running
    }

    pub fn is_live_untracked(&self) -> bool {
        self.is_remote_untracked() || self.status.get_untracked() == RuntimeStatus::Running
    }

    /// The local controller's address (ClashBar's
    /// `localExternalControllerDisplay`).
    pub fn local_controller_display(&self) -> String {
        if !self.is_remote() {
            return self.target.with(|t| t.controller.clone());
        }
        self.selected_config_path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .map(|c| crate::config::api_host_port(&crate::config::launch_controller(&c)))
            .unwrap_or_else(|| "127.0.0.1:9097".into())
    }

    pub fn machine_status(&self, id: &str) -> MachineStatus {
        self.machine_statuses.with(|s| s.get(id).cloned().unwrap_or_default())
    }

    // --- The store -------------------------------------------------------

    pub fn add_machine(&self, machine: RemoteMachine) {
        self.settings.update(|s| s.remote_machines.push(machine));
    }

    pub fn update_machine(&self, machine: RemoteMachine) {
        let id = machine.id.clone();
        self.settings.update(|s| {
            if let Some(slot) = s.remote_machines.iter_mut().find(|m| m.id == id) {
                *slot = machine;
            }
        });
        self.machine_statuses.update(|s| {
            s.insert(id, MachineStatus::Unknown);
        });
    }

    pub fn remove_machine(&self, id: &str) {
        self.machine_statuses.update(|s| {
            s.remove(id);
        });
        self.settings.update(|s| s.remote_machines.retain(|m| m.id != id));
        if self.remote.with_untracked(|r| r.as_ref().is_some_and(|r| r.id == id)) {
            let m = self.clone();
            run_root(async move { m.switch_to_machine(None).await });
        }
    }

    /// Probes `machine` and keeps its status, unless it changed or went
    /// meanwhile (ClashBar's probe generations).
    pub async fn refresh_connectivity(&self, machine: RemoteMachine) -> MachineStatus {
        self.machine_statuses.update(|s| {
            s.insert(machine.id.clone(), MachineStatus::Checking);
        });
        let status = probe(&machine).await;
        if self.settings.with_untracked(|s| s.remote_machines.contains(&machine)) {
            self.machine_statuses.update(|s| {
                s.insert(machine.id.clone(), status.clone());
            });
        }
        status
    }

    pub async fn check_all_connectivity(&self) {
        let machines = self.settings.with_untracked(|s| s.remote_machines.clone());
        join_all(machines.into_iter().map(|machine| self.refresh_connectivity(machine))).await;
    }

    /// Checks every machine each five seconds while the manager shows
    /// (ClashBar's `startPeriodicConnectivityChecks`).
    pub(crate) fn start_machine_checks(&self) {
        let m = self.clone();
        let generation = std::rc::Rc::new(std::cell::Cell::new(0u64));
        effect(move || {
            let shown = m.side_popover.with(|p| {
                p.as_ref().is_some_and(|p| matches!(p.kind, crate::model::SideKind::Machines | crate::model::SideKind::MachineEditor(_)))
            });
            let current = generation.get() + 1;
            generation.set(current);
            if !shown {
                return;
            }
            let (m, generation) = (m.clone(), generation.clone());
            untracked(|| {
                run_root(async move {
                    while generation.get() == current {
                        m.check_all_connectivity().await;
                        timer(Duration::from_secs(5)).await;
                    }
                });
            });
        });
    }

    // --- Switching -------------------------------------------------------

    /// Points the panel at `target` (`None`: this Mac) (ClashBar's
    /// `switchToMachineTarget`).
    pub async fn switch_to_machine(&self, target: Option<RemoteMachine>) {
        if self.switching_machine.get_untracked() {
            return;
        }
        if let Some(machine) = &target {
            if !self.refresh_connectivity(machine.clone()).await.is_connected() {
                return;
            }
        }
        self.switching_machine.set(true);
        self.remote.set(target.clone());

        self.stop_streams();
        self.reset_traffic();
        self.clear_all_logs();
        self.clear_proxy_presentation();
        self.rules.set(Vec::new());
        self.rules_count.set(0);
        self.connections.set(Vec::new());
        self.connections_count.set(0);
        self.runtime_config.set(Default::default());
        self.remote_core.set(None);
        self.version.set(String::new());

        match &target {
            None => {
                self.log_app("info", self.tr("log.remote.switched_to_local"));
                if let Some(path) = self.selected_config_path() {
                    self.apply_controller_from_config(&path);
                } else {
                    self.target.set(ApiTarget { controller: "127.0.0.1:9097".into(), secret: None });
                }
                self.preserve_settings_on_sync.set(true);
            }
            Some(machine) => {
                self.log_app(
                    "info",
                    self.tr_args("log.remote.switched_to_remote", &[&machine.name, &machine.display_address()]),
                );
                self.target.set(machine.api_target());
                self.preserve_settings_on_sync.set(false);
            }
        }

        let local_running = self.status.get_untracked() == RuntimeStatus::Running;
        if target.is_some() || local_running {
            if self.refresh_from_api().await {
                self.api_ready.set(true);
            } else {
                self.api_status.set(ApiStatus::Failed);
            }
        } else {
            self.api_status.set(ApiStatus::Unknown);
            self.preserve_settings_on_sync.set(false);
        }
        if target.is_none() && local_running {
            self.apply_settings_overlay("switch-overlay").await;
            self.refresh_ssid().await;
            self.apply_ssid_strategy().await;
        }
        if self.is_live_untracked() {
            self.refresh_proxy_groups().await;
        }
        self.switching_machine.set(false);
    }

    /// The header's machine menu (ClashBar's `headerConnectionControl`):
    /// this Mac, the remote machines (reachable ones only), and the manager.
    pub async fn show_machine_menu(&self, on_manage: impl Fn() + 'static) {
        use nsh_core::element::desktop::{MenuItem, popup_menu};
        if self.switching_machine.get_untracked() {
            return;
        }
        self.check_all_connectivity().await;
        let active = self.remote.get_untracked().map(|r| r.id);
        // (ClashBar's "本地" section title is left out: it would read as
        // the item below it.)
        let mut items = Vec::new();
        {
            let m = self.clone();
            items.push(MenuItem::new(self.tr("ui.machine.local")).checked(active.is_none()).on_select(move || {
                if m.is_remote_untracked() {
                    let m = m.clone();
                    run_root(async move { m.switch_to_machine(None).await });
                }
            }));
        }
        let machines = self.settings.with_untracked(|s| s.remote_machines.clone());
        if !machines.is_empty() {
            items.push(MenuItem::separator());
            items.push(MenuItem::new(self.tr("ui.machine.remote_label")).disabled(true));
        }
        for machine in machines {
            let selected = active.as_deref() == Some(machine.id.as_str());
            let status = self.machine_status(&machine.id);
            let label = match &status {
                MachineStatus::Connected(_) | MachineStatus::Unknown | MachineStatus::Checking => machine.name.clone(),
                MachineStatus::Failed(reason) => format!("{} — {reason}", machine.name),
            };
            let m = self.clone();
            items.push(
                MenuItem::new(label)
                    .checked(selected)
                    .disabled(!status.is_connected() && !selected)
                    .on_select(move || {
                        if !selected {
                            let m = m.clone();
                            let machine = machine.clone();
                            run_root(async move { m.switch_to_machine(Some(machine)).await });
                        }
                    }),
            );
        }
        items.push(MenuItem::separator());
        items.push(MenuItem::new(self.tr("ui.machine.manage")).on_select(on_manage));
        let _ = popup_menu(items);
    }
}
