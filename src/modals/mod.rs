//! Modal dialog state, input handling, and rendering.
//!
//! Each modal lives in its own submodule with state, input handler,
//! and draw function together.

mod bead_picker;
mod config;
mod help;
mod init;
mod kanban;
mod quit;
mod tool_allow;
mod workers_stream;

pub use bead_picker::{
    BeadPickerItem, BeadPickerState, draw_bead_picker, fetch_bead_picker_data,
    handle_bead_picker_input,
};
pub use config::{ConfigModalState, draw_config_modal, handle_config_modal_input};
pub use help::{HelpContext, draw_help_modal};
pub use init::{InitModalState, draw_init_modal, handle_init_modal_input};
pub use kanban::{
    BeadDetailState, BoardAction, BoardConfig, DepDirection, KanbanBoardState, KanbanFetchMsg,
    draw_kanban_board, handle_kanban_input, load_board_config, stream_board_data,
    watch_beads_directory,
};
pub use quit::draw_quit_modal;
pub use tool_allow::{ToolAllowModalState, draw_tool_allow_modal, handle_tool_allow_modal_input};
pub use workers_stream::{WorkersStreamState, draw_workers_stream, handle_workers_stream_input};
