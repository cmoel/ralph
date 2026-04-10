//! File templates for project initialization.

/// Mode-agnostic agent workflow prompt template.
pub const PROMPT_MD: &str = r#"# Agent Workflow

Complete ONE vertical slice per session. A vertical slice delivers observable value — never build infrastructure without connecting it to user-facing behavior.

## Context Engineering

Your context window is precious. Protect it aggressively.

**Use subagents for:**
- Reading multiple files (spawn parallel readers, get summaries back)
- Searching the codebase (don't let grep results pollute your context)
- Exploring unfamiliar code (let the subagent summarize what matters)
- Any task where you need information but not the raw details

**Keep in your context:**
- The current work item you're implementing
- The code you're actively writing
- Errors you're actively debugging

When in doubt, spawn a subagent. A clean context beats a complete one.

## 1. Discover

Find available work using the mode-specific instructions below. Pick ONE item to work on.

If the work is under-specified (unclear acceptance criteria, vague scope), flag it and exit immediately.

If the work is too big for a single session (multiple unrelated concerns, would touch many files across different domains), flag it and exit immediately.

**Immediately after selecting work:** mark it in progress using the mode-specific instructions.

## 2. Understand

Spawn a subagent to read the work item details. Have it identify:
- What this delivers (user-facing behavior or shippable infrastructure)
- Key implementation requirements
- Dependencies on other code

Use `IMPLEMENTATION_PLAN.md` as scratch space — delete its contents freely.

## 3. Search

Before writing code, spawn parallel subagents to search the codebase:
- Existing implementations you can extend
- Patterns to follow
- Code your changes might affect

Never assume something isn't implemented.

## 4. Implement

Build the vertical slice. Prefer TDD — write tests first, then implement. Use your judgment on when TDD doesn't fit (trivial config changes, pure UI work, etc.).

**For complex implementations:** Break into sub-tasks and use subagents for research-heavy steps. Keep your context focused on the code you're writing.

**If blocked:** Document what failed, why it's blocking, and options to resolve. Then flag it using mode-specific instructions and exit.

## 5. Validate

Before committing, run your project's validation:

```bash
# Run your tests
# Run your linter
# Run your type checker
```

Do not commit until validation passes.

## 6. Commit

When the slice is complete, mark the work item done and commit with a clear message.

## 7. Exit

After committing ONE vertical slice, exit immediately. Do not start another task.

## Philosophy

This workflow follows Shape Up methodology — appetite-driven, vertically-sliced, with clear boundaries. For deeper context, see https://www.ryansinger.co/posts/
"#;

/// Generate beads-mode prompt content based on claim state.
/// Command reference is provided by `bd prime` — this only covers Ralph-specific workflow.
pub fn beads_mode_content(claimed_bead_id: Option<&str>) -> String {
    let work_section = match claimed_bead_id {
        Some(id) => format!(
            r#"## Your Work

Ralph claimed bead `{id}` for you. Run `bd show {id}` to read its full specification.

### Assess Before Building

Research the codebase and the bead before committing to build it.

**First, check the hill status.** Run `bd show {id}` and look for `## Hill: Shaped` in the description.

**If `## Hill: Shaped` is NOT present** (missing, Pending, Climbing, or any other status):
1. Update the bead's hill section with WHY it's not ready: `bd update {id} --description="$(bd show {id} --field=description)\n\n## Hill: Climbing\nNot ready: [specific reasons — missing acceptance criteria, approach not grounded in codebase, no edge cases identified, etc.]"`
2. Flag for human: `bd update {id} --add-label=human`
3. Unclaim: `bd update {id} --status=open --assignee=""`
4. Move on to admin work or exit — do NOT write code for unshaped beads

**Build it** if `## Hill: Shaped` is present and the bead is well-specified.

**Redirect** if the bead is under-specified, wrong, or not what the project needs right now:
1. Update with what's missing: `bd update {id} --notes="WHAT'S MISSING: [what sections are absent and what questions need answering]"`
2. Flag for human: `bd update {id} --add-label=human`
3. Unclaim: `bd update {id} --status=open --assignee=""`
4. Do what the project actually needs instead"#
        ),
        None => "## No Claimed Work\n\n\
            No bead was claimed for this session. Assess the project for admin work."
            .to_string(),
    };

    let admin_intro = if claimed_bead_id.is_some() {
        "After completing or unclaiming your bead, consider whether the project needs \
         housekeeping. These are examples — use your judgment about what's actually needed:"
    } else {
        "Assess what the project needs. These are examples — use your judgment \
         about what's actually needed:"
    };

    format!(
        r#"
# Beads Mode

{work_section}

## Confidence Protocol

Research the codebase and beads before making decisions. Act when confident.

When not confident about a bead or decision:
1. Update the bead with open questions — explain what's missing and why
2. Flag it: `bd update <id> --add-label=human`
3. Move on — the bead becomes a handoff artifact with enough context for a human to act

## Admin Work

{admin_intro}
- Organize orphan beads under epics: `bd orphans`
- Flag under-specified beads: `bd update <id> --add-label=human --notes="Needs shaping: [what's missing]"`
- Close eligible epics: `bd epic close-eligible`
- Surface stale items: `bd stale`

When creating new beads during admin work, always include `## Hill: Pending` in the description and add the `human` label. New beads are NOT ready for work — they need shaping first.

## When Blocked — MANDATORY ESCALATION

If ANYTHING prevents you from completing your work — you MUST do ALL THREE:

1. `bd update <id> --add-label=human` — FLAG IT. This is how the human knows it needs attention.
2. `bd update <id> --notes="BLOCKED: [what failed and why]"` — Document the blocker.
3. Exit immediately — do NOT continue working.

This applies to ALL blockers: git failures, permission denials, missing tools, tests failing outside scope, anything you cannot resolve yourself.

NEVER write a note about being blocked without ALSO adding the human label.
A note without the human label is invisible to the user — the bead will rot in in_progress forever.

## Completing Work

Close any beads you complete.
"#
    )
}

/// Brain dump skill (with YAML frontmatter).
pub const BRAIN_DUMP_SKILL_MD: &str =
    include_str!("../.claude/skills/brain-dump/SKILL.md");

/// Shape skill (with YAML frontmatter).
pub const SHAPE_SKILL_MD: &str =
    include_str!("../.claude/skills/shape/SKILL.md");

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_md_contains_hill_enforcement() {
        let content = beads_mode_content(Some("test-123"));
        assert!(
            content.contains("## Hill: Shaped"),
            "beads mode should reference Hill: Shaped check"
        );
        assert!(
            content.contains("NOT present"),
            "beads mode should instruct what to do when hill status is missing"
        );
    }

    #[test]
    fn beads_mode_admin_includes_hill_for_new_beads() {
        let content = beads_mode_content(Some("test-123"));
        assert!(
            content.contains("## Hill: Pending"),
            "admin section should instruct adding Hill: Pending to new beads"
        );
        assert!(
            content.contains("human"),
            "admin section should instruct adding human label to new beads"
        );
    }

    #[test]
    fn beads_mode_unclaimed_includes_hill_for_new_beads() {
        let content = beads_mode_content(None);
        assert!(
            content.contains("## Hill: Pending"),
            "unclaimed admin section should instruct adding Hill: Pending to new beads"
        );
    }

    #[test]
    fn brain_dump_skill_flags_for_human() {
        assert!(
            BRAIN_DUMP_SKILL_MD.contains("--add-label=human"),
            "brain dump should flag beads for human attention"
        );
    }

    #[test]
    fn brain_dump_skill_uses_ask_user_question() {
        assert!(
            BRAIN_DUMP_SKILL_MD.contains("AskUserQuestion"),
            "brain dump should use AskUserQuestion for extraction"
        );
    }

    #[test]
    fn shape_skill_contains_lifecycle_assessment() {
        assert!(
            SHAPE_SKILL_MD.contains("Raw idea"),
            "shape skill should recognize raw idea lifecycle stage"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Bounded context"),
            "shape skill should recognize bounded context lifecycle stage"
        );
    }

    #[test]
    fn shape_skill_contains_shaping_toolkit() {
        assert!(
            SHAPE_SKILL_MD.contains("Exploring"),
            "shape skill should include exploring technique"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Bounding"),
            "shape skill should include bounding technique"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Slicing"),
            "shape skill should include slicing technique"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Codebase Research"),
            "shape skill should include codebase research technique"
        );
    }

    #[test]
    fn shape_skill_defines_readiness() {
        assert!(
            SHAPE_SKILL_MD.contains("Readiness"),
            "shape skill should define readiness criteria"
        );
        assert!(
            SHAPE_SKILL_MD.contains("--remove-label=human"),
            "shape skill should remove human label when ready"
        );
    }

    #[test]
    fn shape_skill_produces_epics_and_slices() {
        assert!(
            SHAPE_SKILL_MD.contains("Epic"),
            "shape skill should produce epics"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Child Beads"),
            "shape skill should produce child beads as vertical slices"
        );
    }

    #[test]
    fn shape_skill_manages_human_label() {
        assert!(
            SHAPE_SKILL_MD.contains("--add-label=human"),
            "shape skill should flag items for human attention"
        );
        assert!(
            SHAPE_SKILL_MD.contains("--remove-label=human"),
            "shape skill should remove human label when ready"
        );
    }
}
