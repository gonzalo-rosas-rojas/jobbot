//! The local panel lives in its own crate (`jobbot-panel`), which reaches the data through the
//! application's storage ports only. This facade keeps the `jobbot::panel::*` paths and is where
//! the command hands the panel the SQLite files.

use anyhow::{Context, Result};

pub use jobbot_panel::*;

use crate::paths::Paths;
use crate::prep::PrepStore;
use crate::tracker::Tracker;

/// Opens `data/jobbot.db` and `data/prep.db`, creating `data/` and the files when missing.
pub fn open(paths: Paths, clock: Clock) -> Result<Panel> {
    std::fs::create_dir_all(&paths.data_dir)
        .with_context(|| format!("creando {}", paths.data_dir.display()))?;
    let tracker = Tracker::open(&paths.data_dir.join("jobbot.db"))?;
    let store = PrepStore::open(&paths.data_dir.join("prep.db"))?;
    Ok(Panel::new(paths, tracker, store, clock))
}
