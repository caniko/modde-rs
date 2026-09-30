//! Bisect command entrypoint and CLI argument adapters.

use clap::ValueEnum;

use modde_core::BisectResult;

mod candidate;
mod flow;
mod perf;
mod start;

#[cfg(test)]
mod tests;

pub(crate) use flow::complete_launch;
pub use flow::{
    handle_abort, handle_history, handle_mark, handle_retry, handle_run, handle_status,
};
pub use start::handle_start;

/// Persisted with a Library request so a store hook or interrupted-client
/// completion worker can evaluate the same candidate as a direct launch.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct Completion {
    pub session: String,
    pub step: i64,
    pub started_unix_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectOracleArg {
    Manual,
    Crash,
    Perf,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum BisectResultArg {
    Good,
    Bad,
}

impl From<BisectResultArg> for BisectResult {
    fn from(value: BisectResultArg) -> Self {
        match value {
            BisectResultArg::Good => Self::Good,
            BisectResultArg::Bad => Self::Bad,
        }
    }
}
