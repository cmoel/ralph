//! Application state and core logic.

mod output;
mod polling;
mod state;

pub use crate::dolt::DoltServerState;
pub use state::{App, AppStatus, PendingDep};
