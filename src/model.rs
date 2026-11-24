//! The app's state and actions (ClashBar's `AppViewModel`): the core's
//! lifecycle, the selected config, the mode, traffic and memory, logs.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use nsh_base::alert_dialog;
use nsh_core::prelude::*;
use serde::Deserialize;
use tokio::sync::mpsc::unbounded_channel;

use crate::api::{self, ApiTarget, Endpoint};
use crate::config;
use crate::core::{CoreError, CoreEvent, CoreProcess};
use crate::i18n;
use crate::paths::WorkingDirectory;
use crate::settings::{AppLanguage, Settings};
use crate::stream::{self, StreamEvent};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RuntimeStatus {
    Stopped,
    Starting,
    Running,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreAction {
    Idle,
    Starting,
    Stopping,
    Restarting,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApiStatus {
    Unknown,
    Healthy,
    Failed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CoreMode {
    Rule,
    Global,
    Direct,
}

impl CoreMode {
    pub const ALL: [CoreMode; 3] = [CoreMode::Rule, CoreMode::Global, CoreMode::Direct];

    pub fn raw(self) -> &'static str {
        match self {
            Self::Rule => "rule",
            Self::Global => "global",
            Self::Direct => "direct",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.to_ascii_lowercase().as_str() {
            "rule" => Some(Self::Rule),
            "global" => Some(Self::Global),
            "direct" => Some(Self::Direct),
            _ => None,
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Rule => "ui.mode.rule",
            Self::Global => "ui.mode.global",
            Self::Direct => "ui.mode.direct",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootTab {
    Proxy,
    Rules,
    Connections,
    Logs,
    System,
}

impl RootTab {
    pub const ALL: [RootTab; 5] = [RootTab::Proxy, RootTab::Rules, RootTab::Connections, RootTab::Logs, RootTab::System];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Proxy => "ui.tab.proxy",
            Self::Rules => "ui.tab.rules",
            Self::Connections => "ui.tab.connections",
            Self::Logs => "ui.tab.logs",
            Self::System => "ui.tab.system",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Traffic {
    pub up: i64,
    pub down: i64,
}

/// The last minute of traffic (one sample a second) and the totals.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct TrafficHistory {
    pub up: VecDeque<i64>,
    pub down: VecDeque<i64>,
    pub up_total: i64,
    pub down_total: i64,
}

/// Samples the sparkline keeps (ClashBar's `historyMaxPoints`).
pub const HISTORY_MAX_POINTS: usize = 60;

/// What a popover attached beside the panel shows.
#[derive(Clone, Debug, PartialEq)]
pub enum SideKind {
    /// A group's nodes.
    Nodes(String),
    /// The configs (the "switch config" menu).
    Configs,
    /// The form importing a subscription link.
    ImportRemote,
    /// The remote machines (ClashBar's `RemoteMachineManagerView`).
    Machines,
    /// The machine editor: a machine's id, or `None` to add one.
    MachineEditor(Option<String>),
}

/// A popover open beside the panel, and the screen `y` of the middle of
/// the row it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct SidePopover {
    pub kind: SideKind,
    pub anchor_mid_y: f32,
}

/// The core's inbound ports (`/configs`).
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct ProxyPorts {
    pub port: i64,
    pub socks_port: i64,
    pub mixed_port: i64,
}

impl ProxyPorts {
    /// The HTTP and SOCKS ports a terminal uses (each falls back to the
    /// other, then to the mixed port).
    pub fn terminal_ports(&self) -> (i64, i64) {
        let positive = |v: i64| (v > 0).then_some(v);
        let http = positive(self.port).or(positive(self.socks_port)).unwrap_or(self.mixed_port);
        let socks = positive(self.socks_port).or(positive(self.port)).unwrap_or(http);
        (http, socks)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LogSource {
    App,
    Mihomo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LogEntry {
    pub id: u64,
    pub at: SystemTime,
    pub level: String,
    pub source: LogSource,
    pub message: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum StartTrigger {
    Manual,
    Auto,
}


thread_local! {
    /// The app root's owner: what actions run under, so a view that goes
    /// (a popover closing, a tab switching) does not cancel them.
    static ROOT_OWNER: RefCell<Option<Owner>> = const { RefCell::new(None) };
}

/// Runs `future` on the engine thread under the app root (`spawn`, but
/// outliving the view that started it).
pub fn run_root(future: impl std::future::Future<Output = ()> + 'static) {
    let owner = ROOT_OWNER.with_borrow(|o| o.clone());
    match owner.filter(Owner::is_live) {
        Some(owner) => {
            owner.run(|| {
                spawn(future);
            });
        }
        None => {
            spawn(future);
        }
    }
}

/// What the model owns that is no signal.
struct Services {
    wd: WorkingDirectory,
    core: CoreProcess,
    settings_path: PathBuf,
    streams: RefCell<HashMap<&'static str, stream::Stream>>,
    next_log_id: RefCell<u64>,
    alert_keys: RefCell<HashMap<String, std::time::Instant>>,
    /// Mihomo's lines not shown yet: flushed in batches (ClashBar's
    /// `pendingMihomoLogs`, 40 lines or 150 ms).
    pending_mihomo_logs: RefCell<Vec<LogEntry>>,
    mihomo_flush_scheduled: RefCell<bool>,
    app_log_file: crate::data::LogFile,
    mihomo_log_file: crate::data::LogFile,
    /// The level the logs stream was opened with.
    logs_stream_level: RefCell<Option<String>>,
}

#[derive(Clone)]
pub struct Model {
    pub settings: Signal<Settings>,
    pub status: Signal<RuntimeStatus>,
    pub action: Signal<CoreAction>,
    pub api_status: Signal<ApiStatus>,
    pub mode: Signal<Option<CoreMode>>,
    pub traffic: Signal<Traffic>,
    pub memory: Signal<i64>,
    pub version: Signal<String>,
    pub configs: Signal<Vec<String>>,
    pub target: Signal<ApiTarget>,
    pub logs: Signal<VecDeque<LogEntry>>,
    pub startup_error: Signal<Option<String>>,
    pub tab: Signal<RootTab>,
    pub panel_visible: Signal<bool>,
    pub history: Signal<TrafficHistory>,
    pub connections_count: Signal<usize>,
    pub groups: Signal<Vec<crate::proxy::ProxyGroup>>,
    pub delay_samples: Signal<HashMap<String, Vec<i64>>>,
    pub node_types: Signal<HashMap<String, String>>,
    pub providers: Signal<BTreeMap<String, crate::proxy::ProviderDetail>>,
    pub group_latency_loading: Signal<HashSet<String>>,
    pub proxy_latency_testing: Signal<HashSet<(String, String)>>,
    pub provider_updating: Signal<HashSet<String>>,
    pub side_popover: Signal<Option<SidePopover>>,
    pub ports: Signal<ProxyPorts>,
    pub connections: Signal<Vec<crate::data::Connection>>,
    pub rules: Signal<Vec<crate::data::RuleItem>>,
    pub rules_count: Signal<usize>,
    pub rule_providers: Signal<BTreeMap<String, crate::proxy::ProviderDetail>>,
    pub rule_providers_refreshing: Signal<bool>,
    /// The core's `log-level`.
    pub log_level: Signal<String>,
    /// The running core's API has answered (streams open after it).
    pub api_ready: Signal<bool>,
    pub runtime_config: Signal<crate::settings_core::RuntimeConfig>,
    /// The next read of the core's config keeps the kept settings (a start
    /// lays them over it).
    pub preserve_settings_on_sync: Signal<bool>,
    pub settings_feedback: Signal<Option<crate::settings_core::Feedback>>,
    pub settings_syncing: Signal<Option<String>>,
    pub port_edit_generation: Signal<u64>,
    pub geo_update: Signal<crate::settings_core::TaskState>,
    pub core_upgrade: Signal<crate::settings_core::TaskState>,
    pub launch_at_login: Signal<bool>,
    pub system_proxy_enabled: Signal<bool>,
    pub system_proxy_display: Signal<Option<String>>,
    pub proxy_syncing: Signal<bool>,
    /// Why the system proxy did not turn on (shown in its row).
    pub system_proxy_hint: Signal<Option<String>>,
    pub tun_syncing: Signal<bool>,
    /// What to turn on again after the next start: (system proxy, TUN).
    pub pending_recovery: Signal<(bool, bool)>,
    pub exceptions_draft: Signal<Vec<String>>,
    pub exceptions_saved: Signal<Vec<String>>,
    pub new_exception: Signal<String>,
    pub remote_phases: Signal<HashMap<String, crate::subscriptions::RemotePhase>>,
    /// Ticks each minute (the subscriptions' "next update" lines read it).
    pub remote_clock: Signal<i64>,
    /// `Some(true)` online, `Some(false)` offline, `None` not known yet.
    pub network_online: Signal<Option<bool>>,
    /// The core was stopped for the network: it starts again when it is back.
    pub resume_after_network: Signal<bool>,
    pub current_ssid: Signal<Option<String>>,
    pub ssid_authorization: Signal<crate::wifi_types::Authorization>,
    /// The remote machine the panel shows (`None`: this Mac).
    pub remote: Signal<Option<crate::remote::RemoteMachine>>,
    pub machine_statuses: Signal<HashMap<String, crate::remote::MachineStatus>>,
    pub switching_machine: Signal<bool>,
    /// A remote machine's settings as edited (never kept).
    pub remote_core: Signal<Option<crate::settings::CoreSettings>>,
    pub machine_draft: Signal<crate::remote::MachineDraft>,
    services: Rc<Services>,
}

/// The bundled `mihomo.gz`: next to the executable in a packaged app
/// (where `nsh package` puts the crate's assets), else in the source tree.
fn bundled_core_path() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    let candidates = [
        contents.join("Resources/nsh/assets/@mihobar/bin/mihomo.gz"),
        contents.join("Resources/nsh/assets/bin/mihomo.gz"),
        contents.join("Resources/assets/bin/mihomo.gz"),
        contents.join("Resources/bin/mihomo.gz"),
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/bin/mihomo.gz")),
    ];
    candidates.into_iter().find(|p| p.exists())
}

impl Model {
    /// Makes the model and starts what runs from launch (ClashBar's
    /// `start()`): directories, the default config, the saved settings, the
    /// core's events, auto-start.
    pub fn new() -> Self {
        ROOT_OWNER.with_borrow_mut(|o| *o = Some(Owner::current()));
        let wd = WorkingDirectory::default();
        let bootstrap_error = wd.bootstrap().err();
        let seed_error = config::seed_default_config(&wd).err();
        let settings_path = wd.state_dir().join("settings.json");
        let settings = Settings::load(&settings_path);
        let (tx, mut rx) = unbounded_channel();
        let core = CoreProcess::new(wd.clone(), bundled_core_path(), tx);
        core.stop_stale_core();
        crate::core::install_termination_handler(&wd.logs_dir().join("signals.log"));
        let app_log_file = crate::data::LogFile::open(wd.logs_dir().join("mihobar.log"));
        let mihomo_log_file = crate::data::LogFile::open(wd.logs_dir().join("mihomo.log"));
        let services = Rc::new(Services {
            wd,
            core,
            settings_path,
            streams: RefCell::new(HashMap::new()),
            next_log_id: RefCell::new(0),
            alert_keys: RefCell::new(HashMap::new()),
            pending_mihomo_logs: RefCell::new(Vec::new()),
            mihomo_flush_scheduled: RefCell::new(false),
            app_log_file,
            mihomo_log_file,
            logs_stream_level: RefCell::new(None),
        });
        let model = Model {
            settings: signal(settings),
            status: signal(RuntimeStatus::Stopped),
            action: signal(CoreAction::Idle),
            api_status: signal(ApiStatus::Unknown),
            mode: signal(None),
            traffic: signal(Traffic::default()),
            memory: signal(0),
            version: signal(String::new()),
            configs: signal(Vec::new()),
            target: signal(ApiTarget { controller: "127.0.0.1:9097".into(), secret: None }),
            logs: signal(VecDeque::new()),
            startup_error: signal(None),
            tab: signal(RootTab::Proxy),
            panel_visible: signal(false),
            history: signal(TrafficHistory::default()),
            connections_count: signal(0),
            groups: signal(Vec::new()),
            delay_samples: signal(HashMap::new()),
            node_types: signal(HashMap::new()),
            providers: signal(BTreeMap::new()),
            group_latency_loading: signal(HashSet::new()),
            proxy_latency_testing: signal(HashSet::new()),
            provider_updating: signal(HashSet::new()),
            side_popover: signal(None),
            ports: signal(ProxyPorts::default()),
            connections: signal(Vec::new()),
            rules: signal(Vec::new()),
            rules_count: signal(0),
            rule_providers: signal(BTreeMap::new()),
            rule_providers_refreshing: signal(false),
            log_level: signal(String::new()),
            api_ready: signal(false),
            runtime_config: signal(Default::default()),
            preserve_settings_on_sync: signal(false),
            settings_feedback: signal(None),
            settings_syncing: signal(None),
            port_edit_generation: signal(0),
            geo_update: signal(Default::default()),
            core_upgrade: signal(Default::default()),
            launch_at_login: signal(false),
            system_proxy_enabled: signal(false),
            system_proxy_display: signal(None),
            proxy_syncing: signal(false),
            system_proxy_hint: signal(None),
            tun_syncing: signal(false),
            pending_recovery: signal((false, false)),
            exceptions_draft: signal(Vec::new()),
            exceptions_saved: signal(Vec::new()),
            new_exception: signal(String::new()),
            remote_phases: signal(HashMap::new()),
            remote_clock: signal(crate::format::now_secs()),
            network_online: signal(None),
            resume_after_network: signal(false),
            current_ssid: signal(None),
            ssid_authorization: signal(Default::default()),
            remote: signal(None),
            machine_statuses: signal(HashMap::new()),
            switching_machine: signal(false),
            remote_core: signal(None),
            machine_draft: signal(Default::default()),
            services,
        };
        model.log_app(
            "info",
            format!("Mihobar {} started (pid {})", env!("CARGO_PKG_VERSION"), std::process::id()),
        );
        if let Some(e) = bootstrap_error {
            model.log_app("error", model.tr_args("log.working_dir_init_failed", &[&e]));
        }
        if let Some(e) = seed_error {
            model.log_app("error", model.tr_args("log.config.import_local.failed", &[&config::DEFAULT_CONFIG_NAME, &e]));
        }

        // Settings are saved as they change.
        {
            let m = model.clone();
            effect(move || {
                let s = m.settings.get();
                s.save(&m.services.settings_path);
            });
        }

        // The appearance follows the setting (ClashBar's `applyAppAppearance`).
        {
            let m = model.clone();
            effect(move || {
                let mode = m.settings.with(|s| s.appearance_mode);
                set_color_scheme(match mode {
                    crate::settings::AppearanceMode::System => nsh_core::platform::Appearance::System,
                    crate::settings::AppearanceMode::Light => nsh_core::platform::Appearance::Light,
                    crate::settings::AppearanceMode::Dark => nsh_core::platform::Appearance::Dark,
                });
            });
        }
        {
            let m = model.clone();
            run_root(async move { m.refresh_launch_at_login().await });
        }

        spawn(async { stream::remember_runtime() });

        // The core's output and exits.
        {
            let m = model.clone();
            spawn(async move {
                while let Some(event) = rx.recv().await {
                    match event {
                        CoreEvent::Log(line) => m.log_mihomo("info", line),
                        CoreEvent::Terminated(code) => m.handle_core_terminated(code),
                    }
                }
            });
        }

        // The streams what is on screen needs (ClashBar's data acquisition
        // policy): traffic while the panel shows or the menu bar shows the
        // speed; memory on the proxy tab; connections on the proxy and
        // connections tabs; mihomo's logs on the logs tab.
        {
            let m = model.clone();
            effect(move || {
                let running = m.is_live() && m.api_ready.get();
                let panel = m.panel_visible.get();
                let tab = m.tab.get();
                let speed_shown = m.settings.with(|s| s.status_bar_display_mode != crate::settings::StatusBarDisplayMode::IconOnly);
                let level = m.log_level.get();
                untracked(|| {
                    m.sync_stream("traffic", running && (panel || speed_shown));
                    m.sync_stream("memory", running && panel && tab == RootTab::Proxy);
                    m.sync_stream("connections", running && panel && matches!(tab, RootTab::Proxy | RootTab::Connections));
                    let logs_on = running && panel && tab == RootTab::Logs;
                    let level = crate::data::stream_log_level(&level);
                    if logs_on && *m.services.logs_stream_level.borrow() != level {
                        m.services.streams.borrow_mut().remove("logs");
                    }
                    m.sync_stream("logs", logs_on);
                });
            });
        }

        // What a tab shows is read again as it opens (ClashBar's
        // `refreshForActivatedTab`).
        {
            let m = model.clone();
            effect(move || {
                let active = m.panel_visible.get() && m.is_live();
                let tab = m.tab.get();
                if !active {
                    return;
                }
                untracked(|| {
                    let m = m.clone();
                    run_root(async move {
                        match tab {
                            RootTab::Proxy => {
                                m.refresh_proxy_groups().await;
                            }
                            RootTab::Rules => m.refresh_providers_and_rules().await,
                            RootTab::Connections => m.refresh_connections().await,
                            RootTab::Logs => {}
                            RootTab::System => {
                                m.refresh_from_api().await;
                            }
                        }
                    });
                });
            });
        }

        // A hidden panel keeps the last 20 logs (ClashBar's
        // `trimInMemoryLogsForCurrentVisibility`).
        {
            let m = model.clone();
            effect(move || {
                let panel = m.panel_visible.get();
                untracked(|| {
                    m.flush_mihomo_logs();
                    let max = if panel { crate::data::MAX_LOGS_VISIBLE } else { crate::data::MAX_LOGS_HIDDEN };
                    if m.logs.with(|l| l.len() > max) {
                        m.logs.update(|l| l.truncate(max));
                    }
                });
            });
        }

        model.seed_features_on_launch();
        model.start_remote_auto_update();
        model.start_network_monitoring();
        model.start_ssid_monitoring();
        model.start_machine_checks();
        model.reload_configs();
        model.restore_selected_config();
        if let Some(path) = model.selected_config_path() {
            model.apply_controller_from_config(&path);
        }
        if model.settings.get_untracked().auto_start_core {
            let m = model.clone();
            spawn(async move { m.start_core(StartTrigger::Auto).await });
        }
        model
    }

    // --- Strings ---------------------------------------------------------

    pub fn language(&self) -> AppLanguage {
        self.settings.with(|s| s.ui_language)
    }

    /// The localized string for `key` (tracked: it follows the language).
    pub fn tr(&self, key: &str) -> String {
        i18n::tr(self.language(), key, &[])
    }

    pub fn tr_args(&self, key: &str, args: &[&dyn std::fmt::Display]) -> String {
        i18n::tr(self.settings.with_untracked(|s| s.ui_language), key, args)
    }

    // --- Logs ------------------------------------------------------------

    fn new_entry(&self, level: &str, source: LogSource, message: String) -> LogEntry {
        let id = {
            let mut next = self.services.next_log_id.borrow_mut();
            *next += 1;
            *next
        };
        LogEntry { id, at: SystemTime::now(), level: level.to_owned(), source, message }
    }

    /// Adds `entries` (oldest first) at the top of the logs, newest first,
    /// and writes them to their files.
    fn append_log_entries(&self, entries: Vec<LogEntry>) {
        if entries.is_empty() {
            return;
        }
        let max = if self.panel_visible.get_untracked() {
            crate::data::MAX_LOGS_VISIBLE
        } else {
            crate::data::MAX_LOGS_HIDDEN
        };
        let app: Vec<_> = entries
            .iter()
            .filter(|e| e.source == LogSource::App)
            .map(|e| (e.at, e.level.as_str(), e.message.as_str()))
            .collect();
        if !app.is_empty() {
            self.services.app_log_file.append(&app);
        }
        let core: Vec<_> = entries
            .iter()
            .filter(|e| e.source == LogSource::Mihomo)
            .map(|e| (e.at, e.level.as_str(), e.message.as_str()))
            .collect();
        if !core.is_empty() {
            self.services.mihomo_log_file.append(&core);
        }
        self.logs.update(|logs| {
            for e in entries {
                logs.push_front(e);
            }
            logs.truncate(max);
        });
    }

    pub fn log_app(&self, level: &str, message: impl Into<String>) {
        let message = message.into();
        if message.is_empty() {
            return;
        }
        log::info!("[mihobar {level}] {message}");
        let entry = self.new_entry(level, LogSource::App, message);
        self.append_log_entries(vec![entry]);
    }

    /// Buffers a mihomo line; the buffer is shown at 40 lines or after
    /// 150 ms.
    fn log_mihomo(&self, level: &str, message: String) {
        if message.is_empty() {
            return;
        }
        let entry = self.new_entry(level, LogSource::Mihomo, message);
        let count = {
            let mut pending = self.services.pending_mihomo_logs.borrow_mut();
            pending.push(entry);
            pending.len()
        };
        if count >= 40 {
            self.flush_mihomo_logs();
            return;
        }
        if !*self.services.mihomo_flush_scheduled.borrow() {
            *self.services.mihomo_flush_scheduled.borrow_mut() = true;
            let m = self.clone();
            run_root(async move {
                timer(Duration::from_millis(150)).await;
                m.flush_mihomo_logs();
            });
        }
    }

    pub fn flush_mihomo_logs(&self) {
        *self.services.mihomo_flush_scheduled.borrow_mut() = false;
        let entries = std::mem::take(&mut *self.services.pending_mihomo_logs.borrow_mut());
        self.append_log_entries(entries);
    }

    /// Empties the logs and their files (ClashBar's `clearAllLogs`).
    pub fn clear_all_logs(&self) {
        self.services.pending_mihomo_logs.borrow_mut().clear();
        self.logs.set(VecDeque::new());
        self.services.app_log_file.clear();
        self.services.mihomo_log_file.clear();
    }

    /// `[2026-11-24 10:00:00] [MIHOMO] [INFO] message`.
    pub fn formatted_log_entry(&self, e: &LogEntry) -> String {
        let source = match e.source {
            LogSource::App => "MIHOBAR",
            LogSource::Mihomo => "MIHOMO",
        };
        format!("[{}] [{source}] [{}] {}", crate::data::date_time(e.at), e.level.to_uppercase(), e.message)
    }

    /// Copies every log (ClashBar's `copyAllLogs`).
    pub fn copy_all_logs(&self) {
        self.flush_mihomo_logs();
        let (content, count) = self.logs.with_untracked(|l| {
            (l.iter().map(|e| self.formatted_log_entry(e)).collect::<Vec<_>>().join("\n"), l.len())
        });
        set_clipboard(content);
        self.log_app("info", self.tr_args("log.logs.copied_all", &[&count]));
    }

    pub fn copy_log_message(&self, e: &LogEntry) {
        set_clipboard(e.message.clone());
        self.log_app("info", self.tr_args("log.logs.copied_message", &[]));
    }

    pub fn copy_log_entry(&self, e: &LogEntry) {
        set_clipboard(self.formatted_log_entry(e));
        self.log_app("info", self.tr_args("log.logs.copied_entry", &[]));
    }

    // --- Alerts ----------------------------------------------------------

    /// A native alert, at most once in 5 seconds per `dedupe_key`.
    fn present_alert(&self, title: String, message: String, dedupe_key: &str) {
        {
            let mut keys = self.services.alert_keys.borrow_mut();
            let now = std::time::Instant::now();
            if keys.get(dedupe_key).is_some_and(|at| now.duration_since(*at) < Duration::from_secs(5)) {
                return;
            }
            keys.insert(dedupe_key.to_owned(), now);
        }
        let ok = self.tr_args("ui.action.ok", &[]);
        spawn(async move {
            let _ = alert_dialog().title(title).description(message).action("ok", ok).show().await;
        });
    }

    // --- Configs ---------------------------------------------------------

    pub fn config_dir(&self) -> PathBuf {
        self.services.wd.config_dir()
    }

    pub fn reload_configs(&self) {
        let files: Vec<String> = config::scan_config_files(&self.config_dir())
            .iter()
            .filter_map(|p| p.file_name().and_then(|n| n.to_str()).map(str::to_owned))
            .collect();
        self.configs.set(files);
    }

    /// The saved selection, else the config the core last started with,
    /// else the first file.
    fn restore_selected_config(&self) {
        let configs = self.configs.get_untracked();
        let s = self.settings.get_untracked();
        let selected = s
            .selected_config
            .filter(|name| configs.contains(name))
            .or_else(|| {
                s.last_successful_config_path
                    .as_deref()
                    .and_then(|p| Path::new(p).file_name()?.to_str().map(str::to_owned))
                    .filter(|name| configs.contains(name))
            })
            .or_else(|| configs.first().cloned());
        self.settings.update(|s| s.selected_config = selected);
    }

    pub fn selected_config_name(&self) -> Option<String> {
        self.settings.with(|s| s.selected_config.clone())
    }

    pub fn selected_config_path(&self) -> Option<PathBuf> {
        let name = self.settings.with_untracked(|s| s.selected_config.clone())?;
        let path = self.config_dir().join(name);
        path.exists().then_some(path)
    }

    /// Reads the controller and its secret from `config` (ClashBar's
    /// `applyExternalControllerFromSelectedConfigFile`); returns the launch
    /// controller.
    pub(crate) fn apply_controller_from_config(&self, config: &Path) -> String {
        let content = std::fs::read_to_string(config).unwrap_or_default();
        let controller = config::launch_controller(&content);
        let secret = config::yaml_scalar(&content, "secret").filter(|s| !s.trim().is_empty());
        let target = ApiTarget { controller: config::api_host_port(&controller), secret };
        if self.target.get_untracked() != target {
            self.target.set(target);
        }
        controller
    }

    /// Selects config `name`, validating it first and restarting the core
    /// with it when the core runs (ClashBar's `selectConfigFile(named:)`).
    pub async fn select_config(&self, name: String) {
        let previous = self.selected_config_name();
        if previous.as_deref() == Some(name.as_str()) {
            return;
        }
        if !self.configs.get_untracked().contains(&name) {
            self.log_app("error", self.tr_args("log.config.not_found", &[&name]));
            return;
        }
        let path = self.config_dir().join(&name);
        if self.services.core.is_running() {
            if let Some(details) = self.config_validation_failure(&path).await {
                self.handle_config_validation_failure(&path, &details);
                return;
            }
        }
        self.settings.update(|s| s.selected_config = Some(name.clone()));
        self.log_app("info", self.tr_args("log.config.selected", &[&name]));
        if self.services.core.is_running() {
            self.clear_proxy_presentation();
            self.log_app("info", self.tr_args("log.config.changed_restart", &[]));
            self.restart_core().await;
        }
    }

    async fn config_validation_failure(&self, path: &Path) -> Option<String> {
        let core = self.services.core.clone();
        let path = path.to_path_buf();
        match blocking(move || core.validate_config(&path)).await {
            Ok(Ok(())) => None,
            Ok(Err(e)) => {
                let details = self.core_error_message(&e).trim().to_owned();
                Some(if details.is_empty() { self.tr_args("ui.common.unknown", &[]) } else { details })
            }
            Err(e) => Some(e.to_string()),
        }
    }

    fn handle_config_validation_failure(&self, path: &Path, details: &str) {
        let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        self.log_app("error", self.tr_args("log.config.validate_failed", &[&file, &details]));
        let mut message = self.tr_args("app.config.validation_failed.message", &[&file]);
        let lowered = details.to_ascii_lowercase();
        let geo = ["geoip", "geosite", "mmdb", "asn"].iter().any(|k| lowered.contains(k));
        message.push_str("\n\n");
        message.push_str(&self.tr_args(
            if geo { "app.config.validation_failed.hint.geo" } else { "app.config.validation_failed.hint.common" },
            &[],
        ));
        message.push_str("\n\n");
        message.push_str(details);
        self.present_alert(self.tr_args("app.config.validation_failed.title", &[]), message, "config-validation");
    }

    fn core_error_message(&self, error: &CoreError) -> String {
        match error {
            CoreError::BinaryNotFound { expected_directory } => {
                self.tr_args("app.core.error.binary_not_found", &[expected_directory])
            }
            other => other.to_string(),
        }
    }

    // --- Core lifecycle --------------------------------------------------

    pub fn is_running(&self) -> bool {
        self.status.get() == RuntimeStatus::Running
    }

    pub fn is_processing(&self) -> bool {
        self.action.get() != CoreAction::Idle
    }

    /// Starts the core on the selected config (ClashBar's `startCore`).
    pub async fn start_core(&self, trigger: StartTrigger) {
        if self.is_remote_untracked() || self.action.get_untracked() != CoreAction::Idle {
            return;
        }
        if trigger == StartTrigger::Manual {
            self.resume_after_network.set(false);
        }
        self.action.set(CoreAction::Starting);
        self.start_core_inner(trigger).await;
        self.action.set(CoreAction::Idle);
    }

    async fn start_core_inner(&self, trigger: StartTrigger) {
        let Some(path) = self.selected_config_path() else {
            let message = self.tr_args("log.start.no_config", &[]);
            self.report_start_failure(message, trigger, false);
            return;
        };
        if let Some(details) = self.config_validation_failure(&path).await {
            self.handle_config_validation_failure(&path, &details);
            if trigger == StartTrigger::Auto {
                let file = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
                self.startup_error.set(Some(self.tr_args("app.config.validation_failed.startup", &[&file])));
                self.status.set(RuntimeStatus::Stopped);
                self.api_status.set(ApiStatus::Unknown);
            } else {
                self.status.set(RuntimeStatus::Failed);
                self.api_status.set(ApiStatus::Failed);
            }
            return;
        }
        let controller = self.apply_controller_from_config(&path);
        self.status.set(RuntimeStatus::Starting);
        let core = self.services.core.clone();
        let launch_path = path.clone();
        let result = blocking(move || core.start(&launch_path, &controller)).await;
        match result {
            Ok(Ok(_pid)) => self.complete_bootstrap(&path, true).await,
            Ok(Err(e)) => {
                let message = self.tr_args("log.start.failed", &[&self.core_error_message(&e)]);
                self.report_start_failure(message, trigger, true);
            }
            Err(e) => {
                let message = self.tr_args("log.start.failed", &[&e]);
                self.report_start_failure(message, trigger, true);
            }
        }
    }

    fn report_start_failure(&self, message: String, trigger: StartTrigger, mark_failed_on_manual: bool) {
        self.log_app("error", message.clone());
        self.present_alert(self.tr_args("app.core.alert.start_failed.title", &[]), message.clone(), "core-start-failed");
        if trigger == StartTrigger::Auto {
            self.startup_error.set(Some(message));
            self.status.set(RuntimeStatus::Stopped);
            self.api_status.set(ApiStatus::Unknown);
        } else if mark_failed_on_manual {
            self.status.set(RuntimeStatus::Failed);
            self.api_status.set(ApiStatus::Failed);
        }
    }

    /// What follows a start or restart (ClashBar's `completeCoreBootstrap`).
    async fn complete_bootstrap(&self, path: &Path, auto_test_latencies: bool) {
        self.status.set(RuntimeStatus::Running);
        self.api_status.set(ApiStatus::Healthy);
        self.reset_traffic();
        // The first read keeps the kept settings; they are laid over the
        // config next (ClashBar's `preserveLocalSettingsOnNextSync`).
        self.preserve_settings_on_sync.set(true);
        // The API answers once the core has loaded its config.
        for _ in 0..12 {
            if self.refresh_from_api().await {
                self.api_ready.set(true);
                break;
            }
            timer(Duration::from_millis(400)).await;
        }
        self.apply_settings_overlay(if auto_test_latencies { "start-overlay" } else { "restart-overlay" }).await;
        self.restore_core_features().await;
        self.refresh_proxy_groups().await;
        let path = path.display().to_string();
        self.settings.update(|s| s.last_successful_config_path = Some(path));
        self.startup_error.set(None);
        if auto_test_latencies {
            let m = self.clone();
            spawn(async move { m.refresh_all_group_latencies(false).await });
        }
    }

    /// Stops the core for the network: it resumes when the network is back.
    pub(crate) async fn stop_core_keeping_resume(&self) {
        self.stop_core_inner().await;
    }

    /// Stops the core (ClashBar's `stopCore`, manual).
    pub async fn stop_core(&self) {
        self.resume_after_network.set(false);
        self.stop_core_inner().await;
    }

    async fn stop_core_inner(&self) {
        if self.is_remote_untracked() || self.action.get_untracked() != CoreAction::Idle {
            return;
        }
        self.action.set(CoreAction::Stopping);
        self.prepare_feature_recovery().await;
        // The streams close first: the core going would read as their
        // failure.
        self.status.set(RuntimeStatus::Stopped);
        self.stop_streams();
        let core = self.services.core.clone();
        let _ = blocking(move || core.stop()).await;
        self.status.set(RuntimeStatus::Stopped);
        self.api_status.set(ApiStatus::Unknown);
        self.clear_proxy_presentation();
        self.reset_traffic();
        self.action.set(CoreAction::Idle);
    }

    /// Restarts the core on the selected config (ClashBar's `restartCore`).
    pub async fn restart_core(&self) {
        if self.is_remote_untracked() || self.action.get_untracked() != CoreAction::Idle {
            return;
        }
        self.action.set(CoreAction::Restarting);
        self.restart_core_inner().await;
        self.action.set(CoreAction::Idle);
    }

    async fn restart_core_inner(&self) {
        let Some(path) = self.selected_config_path() else {
            let message = self.tr_args("log.start.no_config", &[]);
            self.report_restart_failure(message);
            return;
        };
        if let Some(details) = self.config_validation_failure(&path).await {
            self.handle_config_validation_failure(&path, &details);
            return;
        }
        let controller = self.apply_controller_from_config(&path);
        self.prepare_feature_recovery().await;
        self.stop_streams();
        let core = self.services.core.clone();
        let launch_path = path.clone();
        match blocking(move || core.restart(&launch_path, &controller)).await {
            Ok(Ok(_)) => self.complete_bootstrap(&path, false).await,
            Ok(Err(e)) => self.report_restart_failure(self.tr_args("log.restart.failed", &[&self.core_error_message(&e)])),
            Err(e) => self.report_restart_failure(self.tr_args("log.restart.failed", &[&e])),
        }
    }

    fn report_restart_failure(&self, message: String) {
        self.log_app("error", message.clone());
        self.present_alert(self.tr_args("app.core.alert.restart_failed.title", &[]), message, "core-restart-failed");
    }

    /// Restart when running, else start (the header's primary action).
    pub async fn primary_core_action(&self) {
        if self.status.get_untracked() == RuntimeStatus::Running {
            self.restart_core().await;
        } else {
            self.start_core(StartTrigger::Manual).await;
        }
    }

    fn handle_core_terminated(&self, code: i32) {
        let message = self.tr_args("log.process.terminated", &[&code]);
        self.status.set(RuntimeStatus::Failed);
        if self.is_remote_untracked() {
            // The panel shows a remote machine: only the local state changes.
            self.log_app("error", message);
            return;
        }
        self.api_status.set(ApiStatus::Failed);
        self.reset_traffic();
        self.stop_streams();
        self.log_app("error", message.clone());
        if self.action.get_untracked() == CoreAction::Idle {
            self.present_alert(
                self.tr_args("app.core.alert.process_terminated.title", &[]),
                message,
                "core-process-terminated",
            );
        }
    }

    /// Stops the core and quits.
    pub async fn quit(&self) {
        self.prepare_quit_features().await;
        self.stop_streams();
        let core = self.services.core.clone();
        let _ = blocking(move || core.stop()).await;
        nsh_core::element::desktop::quit();
    }

    // --- API -------------------------------------------------------------

    /// Reads the version and the running config; `false` when the API does
    /// not answer.
    pub async fn refresh_from_api(&self) -> bool {
        #[derive(Deserialize)]
        struct Version {
            version: String,
        }
        let target = self.target.get_untracked();
        let Ok(v) = api::request::<Version>(&target, &Endpoint::Version).await else {
            return false;
        };
        self.version.set(v.version);
        if let Ok(c) = api::request::<crate::settings_core::RuntimeConfig>(&target, &Endpoint::GetConfigs).await {
            self.mode.set(c.mode.as_deref().and_then(CoreMode::parse));
            let level = c.log_level.clone().unwrap_or_default();
            if self.log_level.get_untracked() != level {
                self.log_level.set(level);
            }
            self.ports.set(ProxyPorts {
                port: c.port.unwrap_or(0),
                socks_port: c.socks_port.unwrap_or(0),
                mixed_port: c.mixed_port.unwrap_or(0),
            });
            self.sync_runtime_config(c);
        }
        self.api_status.set(ApiStatus::Healthy);
        true
    }

    pub fn is_mode_switch_enabled(&self) -> bool {
        self.is_live() && self.api_status.get() == ApiStatus::Healthy
    }

    /// Switches the core's mode (ClashBar's `switchMode`).
    pub async fn switch_mode(&self, mode: CoreMode) {
        if self.mode.get_untracked() == Some(mode) {
            return;
        }
        let target = self.target.get_untracked();
        let body = serde_json::json!({ "mode": mode.raw() });
        let action = self.tr_args("log.action_name.switch_mode", &[&self.tr_args(mode.title_key(), &[])]);
        match api::request_no_response(&target, &Endpoint::PatchConfigs(body)).await {
            Ok(()) => {
                self.mode.set(Some(mode));
                let raw = mode.raw().to_owned();
                if self.is_remote_untracked() {
                    self.remote_core.update(|c| {
                        if let Some(c) = c.as_mut() {
                            c.mode = raw;
                        }
                    });
                } else {
                    self.settings.update(|s| {
                        if let Some(c) = s.core_settings.as_mut() {
                            c.mode = raw;
                        }
                    });
                }
                self.log_app("info", self.tr_args("log.action.success", &[&action]));
            }
            Err(e) => self.log_app("error", self.tr_args("log.action.failed", &[&action, &e])),
        }
    }

    pub(crate) fn clear_proxy_presentation(&self) {
        self.mode.set(None);
        self.clear_proxy_data();
        self.side_popover.set(None);
    }

    // --- Streams ---------------------------------------------------------

    pub(crate) fn reset_traffic(&self) {
        self.api_ready.set(false);
        self.traffic.set(Traffic::default());
        self.memory.set(0);
        self.history.set(TrafficHistory::default());
        self.connections_count.set(0);
        self.connections.set(Vec::new());
    }

    /// Opens stream `key` when `on` and it is not open, closes it when not
    /// `on` (ClashBar's `syncStream`).
    fn sync_stream(&self, key: &'static str, on: bool) {
        let open = self.services.streams.borrow().contains_key(key);
        if !on {
            if open {
                self.services.streams.borrow_mut().remove(key);
                if key == "logs" {
                    *self.services.logs_stream_level.borrow_mut() = None;
                }
            }
            return;
        }
        if open {
            return;
        }
        let target = self.target.get_untracked();
        let m = self.clone();
        match key {
            "traffic" => self.open_stream(key, target.ws_url("/traffic", &[]), move |payload: &str| {
                #[derive(Deserialize)]
                struct T {
                    up: i64,
                    down: i64,
                    #[serde(rename = "upTotal", default)]
                    up_total: Option<i64>,
                    #[serde(rename = "downTotal", default)]
                    down_total: Option<i64>,
                }
                if let Ok(t) = serde_json::from_str::<T>(payload) {
                    let next = Traffic { up: t.up, down: t.down };
                    if m.traffic.get_untracked() != next {
                        m.traffic.set(next);
                    }
                    m.history.update(|h| {
                        h.up.push_back(t.up.max(0));
                        h.down.push_back(t.down.max(0));
                        while h.up.len() > HISTORY_MAX_POINTS {
                            h.up.pop_front();
                        }
                        while h.down.len() > HISTORY_MAX_POINTS {
                            h.down.pop_front();
                        }
                        // Totals as the core reports them, else summed from
                        // the rates (a sample a second).
                        match (t.up_total, t.down_total) {
                            (Some(u), Some(d)) => {
                                h.up_total = u.max(0);
                                h.down_total = d.max(0);
                            }
                            _ => {
                                h.up_total += t.up.max(0);
                                h.down_total += t.down.max(0);
                            }
                        }
                    });
                }
            }),
            "memory" => self.open_stream(key, target.ws_url("/memory", &[]), move |payload: &str| {
                #[derive(Deserialize)]
                struct Mem {
                    inuse: i64,
                }
                if let Ok(mem) = serde_json::from_str::<Mem>(payload) {
                    if m.memory.get_untracked() != mem.inuse {
                        m.memory.set(mem.inuse);
                    }
                }
            }),
            "connections" => {
                let url = target.ws_url("/connections", &[("interval", "1000".to_owned())]);
                self.open_stream(key, url, move |payload: &str| m.apply_connections_payload(payload));
            }
            "logs" => {
                let level = crate::data::stream_log_level(&self.log_level.get_untracked());
                let query: Vec<(&str, String)> = level.iter().map(|l| ("level", l.clone())).collect();
                *self.services.logs_stream_level.borrow_mut() = level;
                self.open_stream(key, target.ws_url("/logs", &query), move |payload: &str| {
                    if let Some((level, message)) = crate::data::decode_log_payload(payload) {
                        m.log_mihomo(&level, message);
                    }
                });
            }
            _ => {}
        }
    }

    fn apply_connections_payload(&self, payload: &str) {
        if let Some((list, total)) = crate::data::parse_connections(payload) {
            if self.connections_count.get_untracked() != total {
                self.connections_count.set(total);
            }
            if self.connections.get_untracked() != list {
                self.connections.set(list);
            }
        }
    }

    fn open_stream(&self, key: &'static str, url: String, on_payload: impl Fn(&str) + 'static) {
        let (handle, mut rx) = stream::open(url);
        self.services.streams.borrow_mut().insert(key, handle);
        let m = self.clone();
        spawn(async move {
            let mut last_logged: Option<std::time::Instant> = None;
            while let Some(event) = rx.recv().await {
                match event {
                    StreamEvent::Payload(text) => on_payload(&text),
                    StreamEvent::Disconnected(reason) => {
                        // At most one line in 2 seconds per stream.
                        let now = std::time::Instant::now();
                        if last_logged.is_none_or(|at| now.duration_since(at) >= Duration::from_secs(2)) {
                            last_logged = Some(now);
                            let label = m.tr_args(&format!("app.stream.label.{key}"), &[]);
                            m.log_app("warning", m.tr_args("log.stream.disconnected", &[&label, &reason]));
                        }
                    }
                }
            }
        });
    }

    pub(crate) fn stop_streams(&self) {
        self.services.streams.borrow_mut().clear();
    }
}

impl Model {
    /// Copies the terminal's proxy variables for `host` (ClashBar's
    /// `copyProxyCommand`).
    pub fn copy_proxy_command(&self, host: &str) {
        let (http, socks) = self.ports.get_untracked().terminal_ports();
        let host = host.trim();
        let host = if host.contains(':') && !host.starts_with('[') { format!("[{host}]") } else { host.to_owned() };
        let script = format!(
            "export https_proxy=http://{host}:{http} http_proxy=http://{host}:{http} all_proxy=socks5://{host}:{socks}"
        );
        set_clipboard(script);
        self.log_app("info", self.tr_args("log.proxy_export.copied", &[]));
    }

    /// `127.0.0.1:7897`, the address the terminal command points at.
    pub fn local_proxy_target_display(&self) -> String {
        let (http, _) = self.ports.get().terminal_ports();
        format!("{}:{http}", self.proxy_command_host())
    }

    /// The host the terminal's proxy variables use: a remote machine's,
    /// else this Mac (ClashBar's `managedEndpointProxyCommandHost`).
    pub fn proxy_command_host(&self) -> String {
        self.remote.with(|r| r.as_ref().map(|r| r.host.clone())).unwrap_or_else(|| "127.0.0.1".into())
    }
}

impl Model {
    /// Imports a config file the user picks into the config directory
    /// (ClashBar's `importLocalConfigFile`).
    pub async fn import_local_config(&self) {
        let dir = self.config_dir();
        let mut dialog = nsh_core::file_dialog::OpenDialog::files();
        dialog.filters = vec![nsh_core::file_dialog::FileFilter::new("YAML", ["yaml", "yml"])];
        dialog.title = Some(self.tr_args("ui.quick.import_local_config", &[]));
        dialog.button_label = Some(self.tr_args("ui.action.import", &[]));
        dialog.directory = Some(dir.clone());
        let picked = match nsh_core::element::pick_paths(dialog).await {
            Ok(Some(paths)) => paths,
            Ok(None) => return,
            Err(e) => {
                self.log_app("error", self.tr_args("log.config.import_local.failed", &[&"-", &e]));
                return;
            }
        };
        let Some(source) = picked.into_iter().next() else { return };
        let source_name = source.file_name().and_then(|n| n.to_str()).unwrap_or("").to_owned();
        let Some(file_name) = config::normalized_config_file_name(&source_name, None) else {
            self.log_app("error", self.tr_args("log.config.import.invalid_filename", &[&source_name]));
            return;
        };
        let target = dir.join(&file_name);
        let overwrite = target.exists();
        if overwrite && !self.confirm_overwrite(&file_name).await {
            self.log_app("info", self.tr_args("log.config.import.cancelled", &[&file_name]));
            return;
        }
        let result = std::fs::read(&source)
            .map_err(|e| e.to_string())
            .and_then(|data| config::write_config_data(&data, &target));
        match result {
            Ok(_) => {
                self.log_app("info", self.tr_args("log.config.import_local.success", &[&file_name]));
                self.reload_configs();
                if overwrite && self.selected_config_name().as_deref() == Some(file_name.as_str()) && self.services.core.is_running() {
                    self.reload_config().await;
                }
            }
            Err(e) => self.log_app("error", self.tr_args("log.config.import_local.failed", &[&file_name, &e])),
        }
    }

    async fn confirm_overwrite(&self, file_name: &str) -> bool {
        let answer = nsh_base::alert_dialog()
            .title(self.tr_args("app.config.import.overwrite.title", &[&file_name]))
            .description(self.tr_args("app.config.import.overwrite.message", &[]))
            .destructive("overwrite", self.tr_args("ui.action.overwrite", &[]))
            .cancel("cancel", self.tr_args("ui.action.cancel", &[]))
            .show()
            .await;
        format!("{answer:?}").contains("overwrite")
    }

    /// Reloads the selected config in the running core (ClashBar's
    /// `reloadConfig`: `PUT /configs?force=true`).
    pub async fn reload_config(&self) {
        let Some(path) = self.selected_config_path() else { return };
        let action = self.tr_args("log.action_name.reload_config", &[]);
        let target = self.target.get_untracked();
        let endpoint = Endpoint::PutConfigs { force: true, path: path.display().to_string(), payload: None };
        match api::request_no_response(&target, &endpoint).await {
            Ok(()) => {
                self.log_app("info", self.tr_args("log.action.success", &[&action]));
                self.refresh_from_api().await;
                self.refresh_proxy_groups().await;
            }
            Err(e) => self.log_app("error", self.tr_args("log.action.failed", &[&action, &e])),
        }
    }

    /// Shows the config directory in Finder.
    pub fn show_config_dir(&self) {
        let _ = nsh_core::element::open_path(self.config_dir());
    }
}

impl Model {
    pub fn is_running_untracked(&self) -> bool {
        self.status.get_untracked() == RuntimeStatus::Running
    }

    pub fn core_dir(&self) -> PathBuf {
        self.services.wd.core_dir()
    }
}
