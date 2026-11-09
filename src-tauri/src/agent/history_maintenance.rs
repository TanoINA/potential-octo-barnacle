//! Optional, bounded idle projection maintenance. No timer or new global lock.
//! A batch yields between files; its time budget is cooperative, not a deadline:
//! an individual large canonical file can exceed it. Busy session locks are skipped.
use super::{
    session_pages::{self, WarmIndexResult},
    storage,
};

pub async fn warm_session_index(limit: usize, budget_ms: u64) -> Result<WarmIndexResult, String> {
    tokio::task::spawn_blocking(move || {
        session_pages::warm_index(&storage::config_dir().join("sessions"), limit, budget_ms)
    })
    .await
    .map_err(|e| e.to_string())?
}
