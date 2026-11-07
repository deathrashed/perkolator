//! Resolved global options: home, projects, state dir and target registry.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use perkolator_core::CancelToken;
use perkolator_deployment::{Executor, Manifest};
use perkolator_registry::{Registry, ScanConfig, ScanReport, scan};
use perkolator_targets::{HomeEnv, TargetRegistry};

use crate::CliError;
use crate::cli::GlobalArgs;

/// Default state directory relative to the home directory.
const STATE_SUBDIR: &str = ".local/state/perkolator";
const MANIFEST_FILE: &str = "manifest.json";
const JOURNAL_SUBDIR: &str = "journal";

pub struct Env {
    /// Canonical home directory.
    pub home: PathBuf,
    /// Canonical project roots.
    pub projects: Vec<PathBuf>,
    pub home_env: HomeEnv,
    pub state_dir: PathBuf,
    pub targets: TargetRegistry,
}

/// A scan merged into a registry, with its wall-clock duration.
pub struct Scanned {
    pub report: ScanReport,
    pub registry: Registry,
    pub elapsed: Duration,
}

impl Env {
    pub fn new(global: &GlobalArgs) -> Result<Self, CliError> {
        let (home, home_env) = match &global.home {
            // An explicit home is a sandbox: host override variables from the
            // real environment (CLAUDE_CONFIG_DIR, XDG_CONFIG_HOME, ...) are
            // ignored so nothing resolves outside it.
            Some(dir) => {
                let home = canonical_dir(dir, "--home")?;
                (home.clone(), HomeEnv::new(home))
            }
            None => {
                let env = HomeEnv::from_process_env()
                    .ok_or_else(|| CliError::usage("cannot determine home; pass --home DIR"))?;
                let home = canonical_dir(&env.home, "home directory")?;
                (
                    home.clone(),
                    HomeEnv {
                        home: home.clone(),
                        ..env
                    },
                )
            }
        };
        let projects = global
            .projects
            .iter()
            .map(|project| canonical_dir(project, "--project"))
            .collect::<Result<Vec<_>, _>>()?;
        let state_dir = match &global.state_dir {
            Some(dir) => absolute(dir)?,
            None => home.join(STATE_SUBDIR),
        };
        Ok(Self {
            home,
            projects,
            home_env,
            state_dir,
            targets: TargetRegistry::builtin(),
        })
    }

    pub fn scan(&self) -> Scanned {
        let config = ScanConfig::well_known(&self.home, &self.projects);
        let started = Instant::now();
        let report = scan(&config, &CancelToken::new());
        let registry = Registry::from_report(&report);
        Scanned {
            report,
            registry,
            elapsed: started.elapsed(),
        }
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.state_dir.join(MANIFEST_FILE)
    }

    pub fn journal_dir(&self) -> PathBuf {
        self.state_dir.join(JOURNAL_SUBDIR)
    }

    pub fn load_manifest(&self) -> Result<Manifest, CliError> {
        let path = self.manifest_path();
        Manifest::load(&path).map_err(|error| CliError::io(path.display().to_string(), error))
    }

    pub fn executor(&self) -> Executor {
        Executor::new(self.journal_dir(), self.manifest_path())
    }
}

fn canonical_dir(path: &Path, what: &str) -> Result<PathBuf, CliError> {
    let canonical = path
        .canonicalize()
        .map_err(|error| CliError::io(format!("{what} {}", path.display()), error))?;
    if canonical.is_dir() {
        Ok(canonical)
    } else {
        Err(CliError::usage(format!(
            "{what} {} is not a directory",
            path.display()
        )))
    }
}

/// Absolute path without following a final symlink: the parent is
/// canonicalized when it exists, the last component is kept as given.
pub fn absolute(path: &Path) -> Result<PathBuf, CliError> {
    let absolute = std::path::absolute(path)
        .map_err(|error| CliError::io(path.display().to_string(), error))?;
    let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name()) else {
        return Ok(absolute);
    };
    Ok(match parent.canonicalize() {
        Ok(parent) => parent.join(name),
        Err(_) => absolute,
    })
}
