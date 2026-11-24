//! The mihomo process (ClashBar's `MihomoProcessManager`): finding the
//! binary (the managed copy, or the bundled `mihomo.gz` unpacked into it),
//! validating a config with `mihomo -t`, starting, stopping and watching
//! the process. Blocking calls; the app runs them on blocking threads.

use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc::UnboundedSender;

use crate::paths::WorkingDirectory;

/// What the process reports to the app.
#[derive(Debug, Clone)]
pub enum CoreEvent {
    /// A line of its output, or one of the manager's own.
    Log(String),
    /// It exited without being asked to.
    Terminated(i32),
}

#[derive(Debug)]
pub enum CoreError {
    BinaryNotFound { expected_directory: String },
    Other(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BinaryNotFound { expected_directory } => write!(
                f,
                "mihomo binary not found. Expected an executable named 'mihomo' in {expected_directory}."
            ),
            Self::Other(message) => f.write_str(message),
        }
    }
}

impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Other(e.to_string())
    }
}

/// The running core's pid, for the SIGTERM handler (0: none).
static CORE_PID: AtomicI32 = AtomicI32::new(0);

/// Where the signal handler notes the signal (an open file's descriptor).
static SIGNAL_LOG_FD: AtomicI32 = AtomicI32::new(-1);

extern "C" fn on_terminate_signal(signal: libc::c_int) {
    // Only async-signal-safe calls: note the signal, stop the core, exit.
    let fd = SIGNAL_LOG_FD.load(Ordering::SeqCst);
    if fd >= 0 {
        let message: &[u8] = if signal == libc::SIGHUP {
            b"[signal] SIGHUP: stopping the core and quitting\n"
        } else {
            b"[signal] SIGTERM: stopping the core and quitting\n"
        };
        unsafe { libc::write(fd, message.as_ptr().cast(), message.len()) };
    }
    let pid = CORE_PID.load(Ordering::SeqCst);
    if pid > 0 {
        unsafe { libc::kill(pid, libc::SIGTERM) };
    }
    unsafe { libc::_exit(0) };
}

/// Stops the core when the app is sent SIGTERM (a `kill`, `launchctl`),
/// which ends the app without its quit path.
pub fn install_termination_handler(signal_log: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().create(true).append(true).open(signal_log) {
        use std::os::fd::IntoRawFd;
        SIGNAL_LOG_FD.store(file.into_raw_fd(), Ordering::SeqCst);
    }
    unsafe {
        libc::signal(libc::SIGTERM, on_terminate_signal as *const () as usize);
        libc::signal(libc::SIGHUP, on_terminate_signal as *const () as usize);
    }
}

/// The executable of process `pid`.
fn process_path(pid: i32) -> Option<PathBuf> {
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    if n <= 0 {
        return None;
    }
    buf.truncate(n as usize);
    Some(PathBuf::from(String::from_utf8_lossy(&buf).into_owned()))
}

struct Running {
    pid: u32,
    exited: Arc<AtomicBool>,
    intentional: Arc<AtomicBool>,
}

#[derive(Clone)]
pub struct CoreProcess {
    wd: WorkingDirectory,
    bundled_gz: Option<PathBuf>,
    events: UnboundedSender<CoreEvent>,
    running: Arc<Mutex<Option<Running>>>,
    validation_timeout: Duration,
}

impl CoreProcess {
    fn pid_file(&self) -> PathBuf {
        self.wd.state_dir().join("mihomo.pid")
    }

    /// Stops a core a previous run left behind (the app crashed or was
    /// killed): the pid file's process, when it is this app's binary.
    pub fn stop_stale_core(&self) {
        let pid_file = self.pid_file();
        let Some(pid) = std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse::<i32>().ok()) else {
            return;
        };
        let _ = std::fs::remove_file(&pid_file);
        let managed = self.wd.managed_mihomo_binary();
        let same = |p: &Path| std::fs::canonicalize(p).ok() == std::fs::canonicalize(&managed).ok();
        if pid > 0 && process_path(pid).is_some_and(|p| same(&p)) {
            self.log(format!("[mihomo stop] stale core from a previous run pid={pid}"));
            unsafe { libc::kill(pid, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(2);
            while Instant::now() < deadline && unsafe { libc::kill(pid, 0) } == 0 {
                std::thread::sleep(Duration::from_millis(50));
            }
            if unsafe { libc::kill(pid, 0) } == 0 {
                unsafe { libc::kill(pid, libc::SIGKILL) };
            }
        }
    }

    pub fn new(wd: WorkingDirectory, bundled_gz: Option<PathBuf>, events: UnboundedSender<CoreEvent>) -> Self {
        Self {
            wd,
            bundled_gz,
            events,
            running: Arc::new(Mutex::new(None)),
            validation_timeout: Duration::from_secs(10),
        }
    }

    fn log(&self, line: impl Into<String>) {
        let _ = self.events.send(CoreEvent::Log(line.into()));
    }

    pub fn is_running(&self) -> bool {
        self.running
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|r| !r.exited.load(Ordering::SeqCst))
    }

    pub fn pid(&self) -> Option<u32> {
        self.running.lock().unwrap().as_ref().filter(|r| !r.exited.load(Ordering::SeqCst)).map(|r| r.pid)
    }

    /// The binary's path when it is installed (without installing it).
    pub fn detected_binary_path(&self) -> Option<PathBuf> {
        let managed = self.wd.managed_mihomo_binary();
        managed.exists().then_some(managed)
    }

    pub fn has_bundled_core(&self) -> bool {
        self.bundled_gz.as_ref().is_some_and(|p| p.exists())
    }

    /// The managed binary, unpacked from the bundled `mihomo.gz` when it
    /// is not there yet, after its security checks.
    pub fn resolve_binary(&self) -> Result<PathBuf, CoreError> {
        self.wd.bootstrap()?;
        let managed = self.wd.managed_mihomo_binary();
        if managed.exists() {
            ensure_executable(&managed)?;
            validate_binary_security(&managed)?;
            return Ok(managed);
        }
        let Some(gz) = self.bundled_gz.as_ref().filter(|p| p.exists()) else {
            return Err(CoreError::BinaryNotFound { expected_directory: self.wd.core_dir().display().to_string() });
        };
        validate_binary_security(gz)?;
        let tmp = managed.with_extension("tmp");
        let _ = std::fs::remove_file(&tmp);
        let result = (|| -> std::io::Result<()> {
            let mut decoder = flate2::read::GzDecoder::new(std::io::BufReader::new(std::fs::File::open(gz)?));
            let mut out = std::fs::File::create(&tmp)?;
            std::io::copy(&mut decoder, &mut out)?;
            out.sync_all()?;
            std::fs::rename(&tmp, &managed)
        })();
        if let Err(e) = result {
            let _ = std::fs::remove_file(&tmp);
            return Err(CoreError::Other(format!(
                "failed to decompress bundled mihomo binary from {}: {e}",
                gz.display()
            )));
        }
        self.log(format!("[mihomo binary] decompressed bundled core to {}", managed.display()));
        ensure_executable(&managed)?;
        validate_binary_security(&managed)?;
        Ok(managed)
    }

    /// `mihomo -t` on `config`: `Err` with its output when it fails or
    /// takes longer than the timeout.
    pub fn validate_config(&self, config: &Path) -> Result<(), CoreError> {
        let binary = self.resolve_binary()?;
        let workdir = working_directory_for(config);
        let mut child = Command::new(&binary)
            .current_dir(&workdir)
            .arg("-d")
            .arg(&workdir)
            .arg("-f")
            .arg(config)
            .arg("-t")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| CoreError::Other(format!("Failed to run mihomo -t: {e}")))?;
        let output = Arc::new(Mutex::new(String::new()));
        let readers: Vec<_> = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
            .map(|mut stream| {
                let output = output.clone();
                std::thread::spawn(move || {
                    let mut buf = String::new();
                    let _ = stream.read_to_string(&mut buf);
                    output.lock().unwrap().push_str(&buf);
                })
            })
            .collect();
        let deadline = Instant::now() + self.validation_timeout;
        let status = loop {
            if let Some(status) = child.try_wait()? {
                break Some(status);
            }
            if Instant::now() >= deadline {
                break None;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        let seconds = self.validation_timeout.as_secs().max(1);
        if status.is_none() {
            self.log(format!("[mihomo config test] timeout after {seconds}s"));
            let _ = child.kill();
            let _ = child.wait();
        }
        for reader in readers {
            let _ = reader.join();
        }
        let text = output.lock().unwrap().trim().to_owned();
        match status {
            None => Err(CoreError::Other(if text.is_empty() {
                format!("mihomo -t timed out after {seconds} seconds.")
            } else {
                format!("mihomo -t timed out after {seconds} seconds.\n{text}")
            })),
            Some(s) if !s.success() => Err(CoreError::Other(if text.is_empty() {
                format!("mihomo -t exited with code {}.", s.code().unwrap_or(-1))
            } else {
                text
            })),
            Some(_) => {
                if !text.is_empty() {
                    self.log(format!("[mihomo config test] {text}"));
                }
                Ok(())
            }
        }
    }

    /// Starts mihomo on `config` with its controller at `controller`;
    /// returns its pid (the running one's when it already runs).
    pub fn start(&self, config: &Path, controller: &str) -> Result<u32, CoreError> {
        if let Some(pid) = self.pid() {
            return Ok(pid);
        }
        let binary = self.resolve_binary()?;
        let workdir = working_directory_for(config);
        let mut child = Command::new(&binary)
            .current_dir(&workdir)
            .arg("-d")
            .arg(&workdir)
            .arg("-f")
            .arg(config)
            .arg("-ext-ctl")
            .arg(controller)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                let reason = format!("Failed to launch mihomo: {e}");
                self.log(format!("[mihomo error] {reason}"));
                CoreError::Other(reason)
            })?;
        let pid = child.id();
        let exited = Arc::new(AtomicBool::new(false));
        let intentional = Arc::new(AtomicBool::new(false));
        // One flood gate for both pipes, as ClashBar has one per pipe.
        for stream in [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>), child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)]
            .into_iter()
            .flatten()
        {
            let events = self.events.clone();
            std::thread::spawn(move || {
                let mut gate = LogFloodGate::default();
                for line in BufReader::new(stream).lines() {
                    let Ok(line) = line else { break };
                    let line = line.trim();
                    if line.is_empty() {
                        continue;
                    }
                    for accepted in gate.accept(line) {
                        if events.send(CoreEvent::Log(accepted)).is_err() {
                            return;
                        }
                    }
                }
            });
        }
        {
            let events = self.events.clone();
            let exited = exited.clone();
            let intentional = intentional.clone();
            let running = self.running.clone();
            let pid_file = self.pid_file();
            std::thread::spawn(move || {
                // The exit code, or the signal that ended it (as
                // Foundation's `terminationStatus` reports it).
                let code = child
                    .wait()
                    .ok()
                    .and_then(|s| s.code().or_else(|| std::os::unix::process::ExitStatusExt::signal(&s)))
                    .unwrap_or(-1);
                exited.store(true, Ordering::SeqCst);
                let _ = CORE_PID.compare_exchange(pid as i32, 0, Ordering::SeqCst, Ordering::SeqCst);
                let _ = std::fs::remove_file(&pid_file);
                let mut slot = running.lock().unwrap();
                let current = slot.as_ref().is_some_and(|r| r.pid == pid);
                if current {
                    *slot = None;
                }
                drop(slot);
                if !current {
                    return;
                }
                if intentional.load(Ordering::SeqCst) {
                    let _ = events.send(CoreEvent::Log(format!("[mihomo stopped] exit={code}")));
                } else {
                    let _ = events.send(CoreEvent::Log(format!("[mihomo terminated] exit={code}")));
                    let _ = events.send(CoreEvent::Terminated(code));
                }
            });
        }
        *self.running.lock().unwrap() = Some(Running { pid, exited, intentional });
        CORE_PID.store(pid as i32, Ordering::SeqCst);
        let _ = std::fs::write(self.pid_file(), pid.to_string());
        self.log(format!(
            "[mihomo started] pid={pid} controller={controller} binary={} workdir={}",
            binary.display(),
            workdir.display()
        ));
        Ok(pid)
    }

    /// Stops the process: SIGTERM, then SIGKILL after 2 seconds.
    pub fn stop(&self) {
        let (pid, exited, intentional) = {
            let slot = self.running.lock().unwrap();
            let Some(r) = slot.as_ref() else { return };
            (r.pid, r.exited.clone(), r.intentional.clone())
        };
        intentional.store(true, Ordering::SeqCst);
        if exited.load(Ordering::SeqCst) {
            return;
        }
        self.log(format!("[mihomo stop] terminate signal sent pid={pid}"));
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
        if wait_for(&exited, Duration::from_secs(2)) {
            return;
        }
        self.log(format!("[mihomo stop] force kill pid={pid}"));
        unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        wait_for(&exited, Duration::from_secs(1));
    }

    pub fn restart(&self, config: &Path, controller: &str) -> Result<u32, CoreError> {
        self.stop();
        self.start(config, controller)
    }
}

fn wait_for(flag: &AtomicBool, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while !flag.load(Ordering::SeqCst) {
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    true
}

/// The `-d` directory for a config: its directory, or that one's parent
/// when it is named `config` (the working directory's layout).
pub fn working_directory_for(config: &Path) -> PathBuf {
    let dir = std::fs::canonicalize(config.parent().unwrap_or(Path::new("/")))
        .unwrap_or_else(|_| config.parent().unwrap_or(Path::new("/")).to_path_buf());
    if dir.file_name().is_some_and(|n| n == "config") {
        dir.parent().map(Path::to_path_buf).unwrap_or(dir)
    } else {
        dir
    }
}

fn ensure_executable(path: &Path) -> Result<(), CoreError> {
    let meta = std::fs::metadata(path)
        .map_err(|_| CoreError::Other(format!("mihomo binary not found at {}", path.display())))?;
    if meta.permissions().mode() & 0o111 == 0 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn validate_binary_security(path: &Path) -> Result<(), CoreError> {
    let link = std::fs::symlink_metadata(path)?;
    if link.file_type().is_symlink() {
        return Err(CoreError::Other(format!("mihomo binary path must not be a symbolic link: {}", path.display())));
    }
    if !link.is_file() {
        return Err(CoreError::Other(format!("mihomo binary must be a regular file: {}", path.display())));
    }
    let uid = unsafe { libc::getuid() };
    if link.uid() != 0 && link.uid() != uid {
        return Err(CoreError::Other(format!("mihomo binary owner must be current user or root: {}", path.display())));
    }
    if link.mode() & 0o022 != 0 {
        return Err(CoreError::Other(format!(
            "mihomo binary permissions are too permissive (writable by group/others): {}",
            path.display()
        )));
    }
    Ok(())
}

/// At most 200 lines a second, repeats dropped (ClashBar's `LogFloodGate`).
#[derive(Default)]
struct LogFloodGate {
    window_start: Option<Instant>,
    emitted: usize,
    dropped: usize,
    last: Option<String>,
}

impl LogFloodGate {
    fn accept(&mut self, line: &str) -> Vec<String> {
        let now = Instant::now();
        let mut out = Vec::new();
        if self.window_start.is_none_or(|start| now - start >= Duration::from_secs(1)) {
            if self.dropped > 0 {
                out.push(format!("[mihomo log] dropped {} flooding lines", self.dropped));
            }
            self.window_start = Some(now);
            self.emitted = 0;
            self.dropped = 0;
            self.last = None;
        }
        if self.last.as_deref() == Some(line) || self.emitted >= 200 {
            self.dropped += 1;
            return out;
        }
        self.last = Some(line.to_owned());
        self.emitted += 1;
        out.push(line.to_owned());
        out
    }
}
