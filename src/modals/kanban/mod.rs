//! Kanban board modal — pipeline-based work board view.
//!
//! Columns are defined in `board_columns.toml`. Each column has a name and a list
//! of shell pipeline sources that return JSON arrays. Ralph renders the results
//! with zero knowledge of beads internals.

mod input;
mod overlays;
mod pipeline;
mod preview;
mod render;
mod state;

pub use input::handle_kanban_input;
pub use pipeline::{stream_board_data, watch_beads_directory};
pub use render::draw_kanban_board;
pub use state::{
    BeadDetailState, BoardAction, BoardConfig, DepDirection, KanbanBoardState, KanbanFetchMsg,
    load_board_config,
};
