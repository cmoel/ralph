//! Configuration modal — settings editor for per-project config.

mod render;
mod state;

pub use render::{draw_config_modal, handle_config_modal_input};
pub use state::{ConfigModalField, ConfigModalState};
