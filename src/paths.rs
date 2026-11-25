//! The app's working directory (ClashBar's `WorkingDirectoryManager`):
//! NSH's data directory for the app (`data_dir()`), which `nsh.json`'s
//! `"dataDir"` keeps at `~/Library/Application Support/mihobar` (where
//! Mihobar always kept it; NSH's own default would be a folder named after
//! the app id, `dev.mihobar.app`, which Finder takes for an app), and its
//! subdirectories.

use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Clone, Debug)]
pub struct WorkingDirectory {
    root: PathBuf,
}

impl Default for WorkingDirectory {
    /// NSH's data directory, or `$MIHOBAR_HOME` (tests and development runs
    /// keep their data apart with it).
    fn default() -> Self {
        if let Some(dir) = std::env::var_os("MIHOBAR_HOME").filter(|d| !d.is_empty()) {
            return Self { root: PathBuf::from(dir) };
        }
        let root = nsh_core::prelude::data_dir()
            .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Application Support/mihobar")))
            .unwrap_or_else(|| PathBuf::from("mihobar"));
        Self { root }
    }
}

impl WorkingDirectory {
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn config_dir(&self) -> PathBuf {
        self.root.join("config")
    }
    pub fn logs_dir(&self) -> PathBuf {
        self.root.join("logs")
    }
    pub fn state_dir(&self) -> PathBuf {
        self.root.join("state")
    }
    pub fn icon_dir(&self) -> PathBuf {
        self.root.join("icon")
    }
    pub fn core_dir(&self) -> PathBuf {
        self.root.join("core")
    }
    pub fn managed_mihomo_binary(&self) -> PathBuf {
        self.core_dir().join("mihomo")
    }

    /// Creates the root and its subdirectories; a file where a directory
    /// should be is an error.
    pub fn bootstrap(&self) -> io::Result<()> {
        for dir in [
            self.root.clone(),
            self.config_dir(),
            self.logs_dir(),
            self.state_dir(),
            self.icon_dir(),
            self.core_dir(),
        ] {
            match std::fs::metadata(&dir) {
                Ok(meta) if meta.is_dir() => {}
                Ok(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!("Expected directory but found file: {}", dir.display()),
                    ));
                }
                Err(_) => std::fs::create_dir_all(&dir)?,
            }
        }
        Ok(())
    }

    /// `path`, normalized, when it stays inside the root.
    pub fn within_root(&self, path: &Path) -> io::Result<PathBuf> {
        let normalized = normalize(path);
        if normalized.starts_with(normalize(&self.root)) {
            Ok(normalized)
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!("Path escapes Mihobar working directory: {}", normalized.display()),
            ))
        }
    }
}

/// Lexical normalization (`.` and `..` resolved), as `standardizedFileURL`.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
