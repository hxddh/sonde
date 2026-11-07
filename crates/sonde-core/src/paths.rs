//! Where sonde keeps its files.
//!
//! - configuration: `$SONDE_CONFIG_DIR`, else `$XDG_CONFIG_HOME/sonde`,
//!   else `~/.config/sonde` on Linux and `~/Library/Application Support/sonde`
//!   on macOS (`config.toml`);
//! - data (request log database, scan cache, probe runs, audit log, plugins):
//!   `$SONDE_DATA_DIR`, else the platform's data directory (`~/.local/share/sonde`,
//!   `~/Library/Application Support/sonde`);
//! - exports and downloads: `exports/` and `downloads/` under the data
//!   directory unless the settings name others.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
}

impl Paths {
    pub fn from_env() -> Paths {
        let config_dir = std::env::var_os("SONDE_CONFIG_DIR")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("XDG_CONFIG_HOME").map(|p| PathBuf::from(p).join("sonde")))
            .or_else(|| dirs::config_dir().map(|p| p.join("sonde")))
            .unwrap_or_else(|| PathBuf::from(".sonde"));
        let data_dir = std::env::var_os("SONDE_DATA_DIR")
            .map(PathBuf::from)
            .or_else(|| dirs::data_dir().map(|p| p.join("sonde")))
            .unwrap_or_else(|| config_dir.join("data"));
        Paths {
            config_dir,
            data_dir,
        }
    }

    /// Both under one directory (tests, portable installs).
    pub fn rooted(root: &Path) -> Paths {
        Paths {
            config_dir: root.join("config"),
            data_dir: root.join("data"),
        }
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }
    pub fn audit_log(&self) -> PathBuf {
        self.data_dir.join("audit.jsonl")
    }
    pub fn reqlog_db(&self) -> PathBuf {
        self.data_dir.join("requests.sqlite")
    }
    pub fn scan_cache(&self) -> PathBuf {
        self.data_dir.join("scans")
    }
    pub fn probe_runs(&self) -> PathBuf {
        self.data_dir.join("probe-runs")
    }
    /// Recorded sessions (`sonde run`, `sonde proxy`).
    pub fn sessions(&self) -> PathBuf {
        self.data_dir.join("sessions")
    }
    pub fn plugins(&self) -> PathBuf {
        self.data_dir.join("plugins")
    }
    pub fn default_exports(&self) -> PathBuf {
        self.data_dir.join("exports")
    }
    pub fn default_downloads(&self) -> PathBuf {
        dirs::download_dir()
            .map(|d| d.join("sonde"))
            .unwrap_or_else(|| self.data_dir.join("downloads"))
    }
}
