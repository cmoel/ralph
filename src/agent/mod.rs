//! Agent lifecycle, worktrees, stale recovery, and epic selection.

mod epic;
mod lifecycle;
mod stale;
mod worktree;

pub use epic::{
    IterationAction, build_dirty_worktree_context, check_worktree_dirty, claim_next_child,
    complete_epic, decide_iteration_action, resolve_worktree_name, select_and_claim_work,
};
pub use lifecycle::{cleanup, register, release_bead, start_heartbeat};
pub use stale::{ResumeResult, find_stale_agents, release_stale_bead, resume_stale_bead};
pub use worktree::{
    create_or_reuse_worktree, escalate_merge_conflict, file_merge_conflict_bead,
    find_merge_conflict_bead, merge_worktree_to_main, remove_merged_worktree,
};
