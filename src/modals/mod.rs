//! Modal dialog state, input handling, and rendering.
//!
//! Each modal lives in its own submodule with state, input handler,
//! and draw function together.

mod config;
mod help;
mod init;
mod kanban;
mod quit;
mod specs_panel;
mod tool_allow;

pub use config::{ConfigModalState, draw_config_modal, handle_config_modal_input};
pub use help::draw_help_modal;
pub use init::{InitModalState, draw_init_modal, handle_init_modal_input};
pub use kanban::{
    KanbanBoardData, KanbanBoardState, draw_kanban_board, fetch_board_data, handle_kanban_input,
    load_board_config, watch_beads_directory,
};
pub use quit::draw_quit_modal;
pub use specs_panel::{SpecsPanelState, draw_specs_panel, handle_specs_panel_input};
pub use tool_allow::{ToolAllowModalState, draw_tool_allow_modal, handle_tool_allow_modal_input};
