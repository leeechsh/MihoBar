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

/// The panel's tabs: nodes and the quick settings. The rest lives in the
/// management window ([`ManagerSection`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RootTab {
    Proxy,
    System,
}

/// How the advanced window's own process was started (by the panel's
/// process, in the app bundle): the core it reads and its first section.
/// It lives only while its window is open, so closing the window gives all
/// of its memory back.
#[derive(Clone, Debug)]
pub struct AdvancedLaunch {
    pub controller: String,
    pub secret: Option<String>,
    pub section: ManagerSection,
}

impl AdvancedLaunch {
    const SECTION: &str = "MIHOBAR_ADVANCED";
    const CONTROLLER: &str = "MIHOBAR_ADVANCED_CONTROLLER";
    const SECRET: &str = "MIHOBAR_ADVANCED_SECRET";

    /// This process is the advanced window's.
    pub fn from_env() -> Option<Self> {
        let section = ManagerSection::from_key(&std::env::var(Self::SECTION).ok()?)?;
        let controller = std::env::var(Self::CONTROLLER).ok().filter(|c| !c.is_empty())?;
        let secret = std::env::var(Self::SECRET).ok().filter(|s| !s.is_empty());
        Some(Self { controller, secret, section })
    }

    fn command(&self, exe: &std::path::Path) -> std::process::Command {
        let mut command = std::process::Command::new(exe);
        command
            .env(Self::SECTION, self.section.key())
            .env(Self::CONTROLLER, &self.controller)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        match &self.secret {
            Some(secret) => command.env(Self::SECRET, secret),
            None => command.env_remove(Self::SECRET),
        };
        command
    }
}

/// The advanced window's sections, each mounted only while it shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ManagerSection {
    Connections,
    Usage,
    Logs,
    Rules,
}

impl ManagerSection {
    pub const ALL: [ManagerSection; 4] = [Self::Connections, Self::Usage, Self::Logs, Self::Rules];

    pub fn key(self) -> &'static str {
        match self {
            Self::Rules => "rules",
            Self::Connections => "connections",
            Self::Usage => "usage",
            Self::Logs => "logs",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.key() == key)
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Rules => "z.rules",
            Self::Connections => "z.connections",
            Self::Usage => "z.usage",
            Self::Logs => "z.logs",
        }
    }
}

impl RootTab {
    pub const ALL: [RootTab; 2] = [RootTab::Proxy, RootTab::System];

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Proxy => "ui.tab.proxy",
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
    /// The open management window, to bring forward.
    pub(crate) manager_window: RefCell<Option<nsh_core::element::desktop::WindowRef>>,
    /// This process is the advanced window's (see [`AdvancedLaunch`]).
    advanced: bool,
    /// Usage counting (the menu bar process only; see `usage`).
    usage: RefCell<Option<crate::usage::Recorder>>,
    /// The connections stream's interval, ms (0: closed).
    connections_interval: std::cell::Cell<u64>,
    /// The advanced window's speeds and closed connections.
    z_tracker: RefCell<crate::conns::Tracker>,
    /// Hides the panel (set by the app's root).
    panel_hide: RefCell<Option<Rc<dyn Fn()>>>,
    /// The config feedback shown now (a newer one replaces it).
    feedback_token: std::cell::Cell<u64>,
    /// Log lines waiting for the next flush (every 500 ms), and the next
    /// line's number.
    z_log_pending: RefCell<Vec<crate::conns::ZLog>>,
    z_log_seq: std::cell::Cell<u64>,
    /// The advanced window's process, started from the panel's.
    advanced_child: RefCell<Option<std::process::Child>>,
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
    /// The management window is open, showing `manager_section`.
    pub manager_open: Signal<bool>,
    pub manager_section: Signal<ManagerSection>,
    /// The menu bar process counts usage (the usage settings' tracking).
    pub usage_tracking: Signal<bool>,
    /// The core's upload and download since it started (bytes).
    pub connection_totals: Signal<(u64, u64)>,
    /// The advanced window's connections: active, closed (the last 500),
    /// frozen while paused.
    pub z_active: Signal<Rc<Vec<crate::conns::Conn>>>,
    pub z_closed: Signal<Rc<Vec<crate::conns::Conn>>>,
    pub z_paused: Signal<bool>,
    /// The advanced window's core logs, newest first.
    pub z_logs: Signal<Rc<Vec<crate::conns::ZLog>>>,
    pub z_logs_paused: Signal<bool>,
    /// The advanced window's own settings (`state/advanced.json`).
    pub z_settings: Signal<crate::conns::AdvancedSettings>,
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
    /// The side popover's measured content height (0: not yet).
    pub side_measured: Signal<f32>,
    /// What the config menu last did (text, failed), shown for a while.
    pub config_feedback: Signal<Option<(String, bool)>>,
    /// The config menu's action running (its label key).
    pub config_busy: Signal<Option<&'static str>>,
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
    /// The latest mihomo release's version, once checked (`updates`).
    pub core_latest: Signal<Option<String>>,
    /// The rows of Settings → Updates: their checks' outcomes.
    pub app_check: Signal<crate::updates::CheckState>,
    pub core_check: Signal<crate::updates::CheckState>,
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
        Self::build(None)
    }

    /// The advanced window's process: no core of its own, no settings or
    /// log files written; it reads the panel's core (see [`AdvancedLaunch`]).
    pub fn new_advanced(launch: AdvancedLaunch) -> Self {
        Self::build(Some(launch))
    }

    fn build(launch: Option<AdvancedLaunch>) -> Self {
        let advanced = launch.is_some();
        ROOT_OWNER.with_borrow_mut(|o| *o = Some(Owner::current()));
        let wd = WorkingDirectory::default();
        let bootstrap_error = wd.bootstrap().err();
        let seed_error = config::seed_default_config(&wd).err();
        let settings_path = wd.state_dir().join("settings.json");
        let wd_state = wd.state_dir();
        let settings = Settings::load(&settings_path);
        let (tx, mut rx) = unbounded_channel();
        let core = CoreProcess::new(wd.clone(), bundled_core_path(), tx);
        if !advanced {
            core.stop_stale_core();
            crate::core::install_termination_handler(&wd.logs_dir().join("signals.log"));
        }
        let app_log_file = crate::data::LogFile::open(wd.logs_dir().join("mihobar.log"));
        let usage = (!advanced).then(|| crate::usage::Recorder::new(wd.root().join("usage")));
        let usage_tracking = usage.as_ref().is_some_and(crate::usage::Recorder::tracking);
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
            manager_window: RefCell::new(None),
            advanced,
            advanced_child: RefCell::new(None),
            usage: RefCell::new(usage),
            connections_interval: std::cell::Cell::new(0),
            z_tracker: RefCell::new(Default::default()),
            panel_hide: RefCell::new(None),
            feedback_token: std::cell::Cell::new(0),
            z_log_pending: RefCell::new(Vec::new()),
            z_log_seq: std::cell::Cell::new(1),
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
            manager_open: signal(false),
            usage_tracking: signal(usage_tracking),
            connection_totals: signal((0, 0)),
            z_active: signal(Rc::new(Vec::new())),
            z_closed: signal(Rc::new(Vec::new())),
            z_paused: signal(false),
            z_logs: signal(Rc::new(Vec::new())),
            z_logs_paused: signal(false),
            z_settings: signal(if advanced { crate::conns::AdvancedSettings::load(&wd_state.join("advanced.json")) } else { Default::default() }),
            manager_section: signal(ManagerSection::Connections),
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
            side_measured: signal(0.0),
            config_feedback: signal(None),
            config_busy: signal(None),
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
            core_latest: signal(None),
            app_check: signal(Default::default()),
            core_check: signal(Default::default()),
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
            services,
        };
        if !advanced {
            model.log_app(
                "info",
                format!("MihoBar {} started (pid {})", crate::updates::app_version(), std::process::id()),
            );
        }
        if let Some(e) = bootstrap_error {
            model.log_app("error", model.tr_args("log.working_dir_init_failed", &[&e]));
        }
        if let Some(e) = seed_error {
            model.log_app("error", model.tr_args("log.config.import_local.failed", &[&config::DEFAULT_CONFIG_NAME, &e]));
        }

        // Settings are saved as they change (by the panel's process only).
        if !advanced {
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
        if !advanced {
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
            // Sockets left open across sleep can hang without erroring:
            // after each wake (NSH's `wake_count`) the open streams start over.
            let last_wake = std::cell::Cell::new(untracked(wake_count));
            effect(move || {
                let wake = wake_count();
                let running = m.is_live() && m.api_ready.get();
                let panel = m.panel_visible.get();
                let tab = m.tab.get();
                let section = m.manager_open.get().then(|| m.manager_section.get());
                let speed_shown = m.settings.with(|s| s.status_bar_display_mode != crate::settings::StatusBarDisplayMode::IconOnly);
                // The advanced window's logs come at its own level.
                let level = if m.services.advanced || m.manager_open.get() { m.z_settings.with(|s| s.log_level.clone()) } else { m.log_level.get() };
                let tracking = m.usage_tracking.get();
                untracked(|| {
                    if last_wake.replace(wake) != wake {
                        m.stop_streams();
                    }
                    let nodes_shown = panel && tab == RootTab::Proxy;
                    let advanced = m.services.advanced;
                    // The advanced window's sidebar shows the speeds and memory.
                    m.sync_stream("traffic", running && (advanced || panel || speed_shown));
                    m.sync_stream("memory", running && (advanced || nodes_shown));
                    // The node tab shows the count, the connections section the
                    // list, the advanced window's sidebar the count; usage is
                    // counted from it every 5 s when nothing shows it.
                    let shown = nodes_shown || section == Some(ManagerSection::Connections) || advanced;
                    let interval = if shown { 1000 } else { 5000 };
                    let on = running && (shown || (tracking && !advanced));
                    if on && m.services.connections_interval.get() != interval {
                        m.sync_stream("connections", false);
                        m.services.connections_interval.set(interval);
                    }
                    m.sync_stream("connections", on);
                    let logs_on = running && section == Some(ManagerSection::Logs);
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
                            RootTab::System => {
                                m.refresh_from_api().await;
                            }
                        }
                    });
                });
            });
        }

        // What a management section shows is read again as it opens.
        {
            let m = model.clone();
            effect(move || {
                let section = m.manager_open.get().then(|| m.manager_section.get());
                if !m.is_live() {
                    return;
                }
                untracked(|| {
                    let m = m.clone();
                    run_root(async move {
                        match section {
                            Some(ManagerSection::Rules) => m.refresh_providers_and_rules().await,
                            Some(ManagerSection::Connections) => m.refresh_connections().await,
                            Some(ManagerSection::Logs | ManagerSection::Usage) | None => {}
                        }
                    });
                });
            });
        }

        // Lists live only while the section that shows them is open: the
        // connections and rules are dropped as their section goes (read
        // again as it opens), and the logs keep their last 200 (ClashBar's
        // `trimInMemoryLogsForCurrentVisibility`). Once the management
        // window or the panel goes, what they held goes back to the
        // system.
        {
            let m = model.clone();
            effect(move || {
                let panel = m.panel_visible.get();
                let open = m.manager_open.get();
                let section = open.then(|| m.manager_section.get());
                untracked(|| {
                    m.flush_mihomo_logs();
                    let max = if section == Some(ManagerSection::Logs) { crate::data::MAX_LOGS_VISIBLE } else { crate::data::MAX_LOGS_HIDDEN };
                    if m.logs.with(|l| l.len() > max) {
                        m.logs.update(|l| {
                            l.truncate(max);
                            l.shrink_to_fit();
                        });
                    }
                    if section != Some(ManagerSection::Connections) && m.connections.with(|c| !c.is_empty()) {
                        m.connections.set(Vec::new());
                    }
                    if section != Some(ManagerSection::Rules) && m.rules.with(|r| !r.is_empty()) {
                        m.rules.set(Vec::new());
                    }
                    if !panel || !open {
                        let m = m.clone();
                        run_root(async move {
                            timer(Duration::from_secs(2)).await;
                            if !m.panel_visible.get_untracked() || !m.manager_open.get_untracked() {
                                release_free_memory();
                            }
                        });
                    }
                });
            });
        }

        if let Some(launch) = launch {
            // Its settings are saved as they change.
            let m = model.clone();
            let path = m.services.wd.state_dir().join("advanced.json");
            effect(move || m.z_settings.with(|s| s.save(&path)));
            model.start_advanced(launch);
            return model;
        }

        // Usage: buffered buckets are written and the advanced window's
        // settings (tracking, retention, clearing) followed every 30 s, also
        // while the stream is closed.
        {
            let m = model.clone();
            run_root(async move {
                loop {
                    timer(Duration::from_secs(30)).await;
                    let tracking = m.services.usage.borrow_mut().as_mut().map(|usage| {
                        usage.flush(crate::format::now_secs());
                        usage.tracking()
                    });
                    if let Some(tracking) = tracking.filter(|t| *t != m.usage_tracking.get_untracked()) {
                        m.usage_tracking.set(tracking);
                    }
                }
            });
        }

        model.seed_features_on_launch();
        model.start_remote_auto_update();
        model.start_network_monitoring();
        model.start_ssid_monitoring();
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

    /// The management window is open on `section`.
    pub fn viewing(&self, section: ManagerSection) -> bool {
        self.manager_open.get_untracked() && self.manager_section.get_untracked() == section
    }

    /// Starts the advanced window's process, or brings it forward when it
    /// runs.
    fn open_advanced_process(&self, exe: &std::path::Path) {
        let mut child = self.services.advanced_child.borrow_mut();
        if let Some(running) = child.as_mut() {
            if matches!(running.try_wait(), Ok(None)) {
                activate_process(running.id());
                return;
            }
        }
        let target = self.target.get_untracked();
        let launch = AdvancedLaunch {
            controller: target.controller,
            secret: target.secret,
            section: self.manager_section.get_untracked(),
        };
        match launch.command(exe).spawn() {
            Ok(started) => {
                *child = Some(started);
                drop(child);
                self.reap_advanced_process();
            }
            Err(e) => {
                *child = None;
                drop(child);
                self.log_app("error", format!("Advanced window failed to start: {e}"));
            }
        }
    }

    /// Collects the advanced window's process once it ends (its window
    /// closed), checking every 2 s: unwaited, it stays a zombie until the
    /// window opens again.
    fn reap_advanced_process(&self) {
        let m = self.clone();
        run_root(async move {
            loop {
                timer(Duration::from_secs(2)).await;
                let mut child = m.services.advanced_child.borrow_mut();
                match child.as_mut().map(|c| c.try_wait()) {
                    Some(Ok(None)) => {}
                    // Ended (now reaped), failed to wait, or gone already.
                    _ => {
                        child.take();
                        break;
                    }
                }
            }
        });
    }

    /// Ends the advanced window's process (as the app quits).
    fn close_advanced_process(&self) {
        if let Some(mut child) = self.services.advanced_child.borrow_mut().take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }

    /// The advanced window's process: shows `launch.section`, live once
    /// the panel's core answers.
    fn start_advanced(&self, launch: AdvancedLaunch) {
        self.target.set(ApiTarget { controller: launch.controller, secret: launch.secret });
        self.manager_section.set(launch.section);
        self.manager_open.set(true);
        let app_log_len = self.load_z_log_history();
        self.follow_app_log(app_log_len);
        let m = self.clone();
        run_root(async move {
            loop {
                if m.refresh_from_api().await {
                    m.status.set(RuntimeStatus::Running);
                    m.api_ready.set(true);
                    break;
                }
                timer(Duration::from_millis(500)).await;
            }
        });
    }

    /// The logs page's history: the last lines of the core's and MihoBar's
    /// log files at the page's level. The length of `mihobar.log` then.
    fn load_z_log_history(&self) -> u64 {
        let level = self.z_settings.with_untracked(|s| s.log_level.clone());
        let keep = self.z_settings.with_untracked(|s| s.log_retention.max(1));
        let (logs, app_len) = crate::conns::history(&self.services.wd.logs_dir(), &level, keep);
        self.services.z_log_seq.set(logs.first().map_or(1, |l| l.seq + 1));
        self.z_logs.set(Rc::new(logs));
        app_len
    }

    /// MihoBar's lines as the panel's process writes them to `mihobar.log`
    /// (the advanced window is a process of its own), from `offset` on,
    /// checked every second. A shorter file was rotated: read from its start.
    fn follow_app_log(&self, mut offset: u64) {
        let path = self.services.wd.logs_dir().join("mihobar.log");
        let m = self.clone();
        run_root(async move {
            use std::io::{Read, Seek, SeekFrom};
            let mut partial = String::new();
            loop {
                timer(Duration::from_secs(1)).await;
                let Ok(mut file) = std::fs::File::open(&path) else { continue };
                let len = file.metadata().map_or(0, |meta| meta.len());
                if len < offset {
                    offset = 0;
                    partial.clear();
                }
                if len == offset {
                    continue;
                }
                let mut bytes = Vec::new();
                if file.seek(SeekFrom::Start(offset)).is_err() || (&mut file).take(len - offset).read_to_end(&mut bytes).is_err() {
                    continue;
                }
                offset = len;
                partial.push_str(&String::from_utf8_lossy(&bytes));
                let Some(end) = partial.rfind('\n') else { continue };
                let whole: String = partial.drain(..=end).collect();
                for line in whole.lines() {
                    if let Some((at, level, message)) = crate::conns::parse_app_file_line(line) {
                        m.push_z_log(crate::conns::ZLogSource::App, at, level, message);
                    }
                }
            }
        });
    }

    /// A log line for the advanced window (the core's or MihoBar's, at the
    /// page's level): numbered, batched every 500 ms (zashboard's
    /// throttled flush), dropped while paused.
    fn push_z_log(&self, source: crate::conns::ZLogSource, at: i64, level: String, payload: String) {
        let page_level = self.z_settings.with_untracked(|s| s.log_level.clone());
        if source == crate::conns::ZLogSource::App && !crate::conns::level_shown(&level, &page_level) {
            return;
        }
        let seq = self.services.z_log_seq.get();
        self.services.z_log_seq.set(seq + 1);
        if self.z_logs_paused.get_untracked() {
            return;
        }
        let first = {
            let mut pending = self.services.z_log_pending.borrow_mut();
            pending.push(crate::conns::ZLog::new(seq, source, at, level, payload));
            pending.len() == 1
        };
        if first {
            let m = self.clone();
            run_root(async move {
                timer(Duration::from_millis(500)).await;
                let pending = std::mem::take(&mut *m.services.z_log_pending.borrow_mut());
                let keep = m.z_settings.with_untracked(|s| s.log_retention.max(1));
                m.z_logs.update(|logs| {
                    let mut next: Vec<_> = pending.into_iter().rev().chain(logs.iter().cloned()).collect();
                    next.truncate(keep);
                    *logs = Rc::new(next);
                });
            });
        }
    }

    /// The advanced window's pages are showing: its own process, or (out of
    /// a bundle) its window in this one.
    fn z_mode(&self) -> bool {
        self.services.advanced || self.manager_open.get_untracked()
    }

    /// How the app's root hides the panel.
    pub(crate) fn set_panel_hide(&self, hide: Rc<dyn Fn()>) {
        *self.services.panel_hide.borrow_mut() = Some(hide);
    }

    /// Hides the panel (before a dialog of its own: shown as a sheet of the
    /// panel, it went with the panel as the focus left it).
    pub(crate) fn hide_panel(&self) {
        let hide = self.services.panel_hide.borrow().clone();
        if let Some(hide) = hide {
            hide();
        }
    }

    /// Shows `text` in the config menu for 5 s (an error in red).
    pub fn config_feedback_show(&self, text: String, failed: bool) {
        let token = self.services.feedback_token.get() + 1;
        self.services.feedback_token.set(token);
        self.config_feedback.set(Some((text, failed)));
        let m = self.clone();
        run_root(async move {
            timer(Duration::from_secs(5)).await;
            if m.services.feedback_token.get() == token {
                m.config_feedback.set(None);
            }
        });
    }

    /// Reloads the config list and, while the core runs, its config (the
    /// menu's 重载配置文件).
    pub async fn reload_configs_and_core(&self) {
        self.config_busy.set(Some("ui.quick.reload_config_list"));
        self.reload_configs();
        let count = self.configs.get_untracked().len();
        self.log_app("info", self.tr_args("log.config.loaded_count", &[&count]));
        let mut text = self.tr_args("log.config.loaded_count", &[&count]);
        if self.is_running_untracked() {
            self.reload_config().await;
            text = format!("{text} · {}", self.tr("ui.quick.config_reloaded"));
        }
        self.config_busy.set(None);
        self.config_feedback_show(text, false);
    }

    /// The interface is in Chinese.
    pub fn is_zh(&self) -> bool {
        self.settings.with_untracked(|s| s.ui_language).resolved() == AppLanguage::ZhHans
    }

    /// Where usage is recorded (see `usage`).
    pub fn usage_dir(&self) -> PathBuf {
        self.services.wd.root().join("usage")
    }

    /// The management window as it opens (and `None` as it closes).
    pub(crate) fn set_manager_window(&self, window: Option<nsh_core::element::desktop::WindowRef>) {
        *self.services.manager_window.borrow_mut() = window;
    }

    /// Opens the management window (on `section`, or where it was) and
    /// brings it forward.
    pub fn open_manager(&self, section: Option<ManagerSection>) {
        if let Some(section) = section {
            self.manager_section.set(section);
        }
        // In the app bundle, the window is a process of its own.
        if let Some(exe) = bundled_executable() {
            self.side_popover.set(None);
            self.open_advanced_process(&exe);
            return;
        }
        if self.z_logs.with_untracked(|l| l.is_empty()) {
            self.load_z_log_history();
        }
        self.manager_open.set(true);
        self.side_popover.set(None);
        if let Some(window) = self.services.manager_window.borrow().as_ref() {
            window.focus();
        }
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
        let max = if self.viewing(ManagerSection::Logs) {
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
            if !self.services.advanced {
                self.services.app_log_file.append(&app);
            }
        }
        let core: Vec<_> = entries
            .iter()
            .filter(|e| e.source == LogSource::Mihomo)
            .map(|e| (e.at, e.level.as_str(), e.message.as_str()))
            .collect();
        if !core.is_empty() {
            if !self.services.advanced {
                self.services.mihomo_log_file.append(&core);
            }
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
        // The logs page shows MihoBar's lines beside the core's.
        if self.z_mode() {
            self.push_z_log(crate::conns::ZLogSource::App, crate::format::now_secs(), level.to_owned(), message.clone());
        }
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
        let _ = set_clipboard(content);
        self.log_app("info", self.tr_args("log.logs.copied_all", &[&count]));
    }

    pub fn copy_log_message(&self, e: &LogEntry) {
        let _ = set_clipboard(e.message.clone());
        self.log_app("info", self.tr_args("log.logs.copied_message", &[]));
    }

    pub fn copy_log_entry(&self, e: &LogEntry) {
        let _ = set_clipboard(self.formatted_log_entry(e));
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

    /// A profile as the panel names it: its file name without `.yaml` /
    /// `.yml`, unless another profile has the same name (`a.yaml` and
    /// `a.yml` keep theirs).
    pub fn profile_label(&self, file: &str) -> String {
        self.configs.with(|all| crate::config::profile_label(file, all))
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

    /// The core runs (what the panel's live data needs).
    pub fn is_live(&self) -> bool {
        self.status.get() == RuntimeStatus::Running
    }

    pub fn is_live_untracked(&self) -> bool {
        self.status.get_untracked() == RuntimeStatus::Running
    }

    pub fn is_running(&self) -> bool {
        self.status.get() == RuntimeStatus::Running
    }

    pub fn is_processing(&self) -> bool {
        self.action.get() != CoreAction::Idle
    }

    /// Starts the core on the selected config (ClashBar's `startCore`).
    pub async fn start_core(&self, trigger: StartTrigger) {
        if self.action.get_untracked() != CoreAction::Idle {
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
        if self.action.get_untracked() != CoreAction::Idle {
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
        if self.action.get_untracked() != CoreAction::Idle {
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
        // Leaves a trace of every quit (a menu press, ⌘Q, logout).
        self.log_app("info", format!("MihoBar quitting (pid {})", std::process::id()));
        self.close_advanced_process();
        if let Some(usage) = self.services.usage.borrow_mut().as_mut() {
            usage.flush(crate::format::now_secs());
        }
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
                self.settings.update(|s| {
                    if let Some(c) = s.core_settings.as_mut() {
                        c.mode = raw;
                    }
                });
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
                if key == "connections" {
                    if let Some(usage) = self.services.usage.borrow_mut().as_mut() {
                        usage.restart();
                    }
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
                let interval = self.services.connections_interval.get().max(1000);
                let url = target.ws_url("/connections", &[("interval", interval.to_string())]);
                self.open_stream(key, url, move |payload: &str| m.apply_connections_payload(payload));
            }
            "logs" => {
                let level = if self.z_mode() {
                    crate::data::stream_log_level(&self.z_settings.with_untracked(|s| s.log_level.clone()))
                } else {
                    crate::data::stream_log_level(&self.log_level.get_untracked())
                };
                let query: Vec<(&str, String)> = level.iter().map(|l| ("level", l.clone())).collect();
                *self.services.logs_stream_level.borrow_mut() = level;
                self.open_stream(key, target.ws_url("/logs", &query), move |payload: &str| {
                    if let Some((level, message)) = crate::data::decode_log_payload(payload) {
                        if m.z_mode() {
                            m.push_z_log(crate::conns::ZLogSource::Core, crate::format::now_secs(), level, message);
                        } else {
                            m.log_mihomo(&level, message);
                        }
                    }
                });
            }
            _ => {}
        }
    }

    fn apply_connections_payload(&self, payload: &str) {
        let Ok(snapshot) = serde_json::from_str::<serde_json::Value>(payload) else { return };
        let tracking = match self.services.usage.borrow_mut().as_mut() {
            Some(usage) => {
                usage.observe(&snapshot, crate::format::now_secs());
                Some(usage.tracking())
            }
            None => None,
        };
        if let Some(tracking) = tracking.filter(|t| *t != self.usage_tracking.get_untracked()) {
            self.usage_tracking.set(tracking);
        }
        let totals = (
            snapshot.get("uploadTotal").and_then(serde_json::Value::as_u64).unwrap_or(0),
            snapshot.get("downloadTotal").and_then(serde_json::Value::as_u64).unwrap_or(0),
        );
        if self.connection_totals.get_untracked() != totals {
            self.connection_totals.set(totals);
        }
        if self.z_mode() {
            let before = self.z_active.get_untracked();
            let (active, closed) = self.services.z_tracker.borrow_mut().apply(&snapshot, &before);
            if self.connections_count.get_untracked() != active.len() {
                self.connections_count.set(active.len());
            }
            if !self.z_paused.get_untracked() {
                self.z_active.set(Rc::new(active));
                if !closed.is_empty() {
                    self.z_closed.update(|all| {
                        let mut next: Vec<_> = all.iter().cloned().chain(closed).collect();
                        let excess = next.len().saturating_sub(crate::conns::CLOSED_KEPT);
                        next.drain(..excess);
                        *all = Rc::new(next);
                    });
                }
            }
            return;
        }
        {
            let (list, total) = crate::data::connections_of(&snapshot);
            if self.connections_count.get_untracked() != total {
                self.connections_count.set(total);
            }
            // The node tab needs only the count.
            if self.viewing(ManagerSection::Connections) && self.connections.get_untracked() != list {
                self.connections.set(list);
            }
        }
    }

    fn open_stream(&self, key: &'static str, url: String, on_payload: impl Fn(&str) + 'static) {
        let (handle, mut rx) = stream::open(url);
        self.services.streams.borrow_mut().insert(key, handle);
        let m = self.clone();
        // Under the app root: this opens from the stream policy's effect,
        // whose next run would cancel a task it owns (the socket stayed
        // registered as open, so the traffic froze at 0).
        run_root(async move {
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
        let _ = set_clipboard(script);
        self.log_app("info", self.tr_args("log.proxy_export.copied", &[]));
    }

    /// `127.0.0.1:7897`, the address the terminal command points at.
    pub fn local_proxy_target_display(&self) -> String {
        let (http, _) = self.ports.get().terminal_ports();
        format!("127.0.0.1:{http}")
    }
}

impl Model {
    /// Imports a config file the user picks into the config directory
    /// (ClashBar's `importLocalConfigFile`).
    pub async fn import_local_config(&self) {
        // The dialog shows in a window of its own with the panel hidden.
        self.side_popover.set(None);
        self.hide_panel();
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
                self.config_feedback_show(self.tr_args("log.config.import_local.success", &[&file_name]), false);
                self.reload_configs();
                if overwrite && self.selected_config_name().as_deref() == Some(file_name.as_str()) && self.services.core.is_running() {
                    self.reload_config().await;
                }
            }
            Err(e) => {
                let text = self.tr_args("log.config.import_local.failed", &[&file_name, &e]);
                self.log_app("error", text.clone());
                self.config_feedback_show(text, true);
            }
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

/// Hands the pages the allocator holds free back to the system (macOS's
/// `malloc_zone_pressure_relief` on every zone): what a hidden panel freed
/// stops counting against the app.
fn release_free_memory() {
    unsafe extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut std::ffi::c_void, goal: usize) -> usize;
    }
    // SAFETY: a null zone means every zone; a goal of 0 means all it can.
    unsafe { malloc_zone_pressure_relief(std::ptr::null_mut(), 0) };
}

/// This executable, when it runs from an app bundle (where the advanced
/// window is a process of its own).
pub(crate) fn bundled_executable() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let contents = exe.parent()?.parent()?;
    (contents.file_name()? == "Contents" && contents.parent()?.extension()? == "app").then_some(exe)
}

/// Brings process `pid`'s windows forward.
#[cfg(target_os = "macos")]
pub(crate) fn activate_process(pid: u32) {
    use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
    if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid as i32) {
        #[allow(deprecated)]
        app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
    }
}

