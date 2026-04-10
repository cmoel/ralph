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
pub const BRAIN_DUMP_SKILL_MD: &str = r#"---
name: brain-dump
description: "Intense idea extraction session. Use when the user wants to brain dump, dump thoughts, capture ideas, process voice notes, raw notes, or stream of consciousness into structured work items."
---

# Brain Dump

Drain ideas from the user's head through intense, relentless questioning, then file them as rough work items. Most items will need further shaping via /shape.

## Setup

Run `bd list --json` to see existing beads for deduplication context.

---

## Phase 1: Drain

This is the primary phase. Spend most of the session here. Your job: get **everything** out of the user's head.

### Accept Any Input

The user may paste text, use voice transcription, share files, or just start talking. Don't be prescriptive about input format — work with whatever arrives.

### Relentless Extraction

Use askUserQuestion to push the user to extract ALL ideas. Never let them off easy.

**Socratic depth** — dig into what they said:
- "Why does this matter? What breaks if you never build it?"
- "You said [X] — what's the actual problem behind that?"
- "Who hits this pain point? How often?"
- "What are you really trying to solve here?"
- "What would you do if you had to solve this in one day?"

**Exhaustive breadth** — sweep for what they haven't said:
- "What else is rattling around in your head?"
- "You mentioned [X] — does that connect to anything else?"
- "What are you forgetting? What's the thing you keep putting off thinking about?"
- "Any friction points you've just accepted as normal?"
- "What would your users complain about if you asked them right now?"

**CRITICAL: Questions must be creative and context-specific.** Drive every question from what the user actually said. Never fall back on generic templates. Each question should feel like it came from a thinking partner who's been paying attention, not a form to fill out.

### Handle Ambiguity

When the user mentions something that might not be a work item — an observation, a complaint, a half-formed thought — **ask about it** rather than silently skipping:
- "You mentioned [X]. Is that something to build, or just context?"
- "That sounds like it could be its own thing. Should we capture it?"

### Track Ideas as They Emerge

Keep a running mental list of ideas as they surface. Group related ones. Notice when new ideas contradict or overlap with earlier ones.

---

## Phase 2: Refine

Transition naturally when the user runs out of new ideas. Signs: shorter answers, "I think that's it," repeating earlier points.

Don't announce "now entering Phase 2." Just shift:
- "OK, let me play back what I've captured and see if we can tighten some of these up."

### Light Shaping (Shape Up Methodology)

For each captured idea, do enough shaping to make it a useful rough work item:

- **Frame the problem:** One sentence on what this solves and why it matters
- **Identify unknowns:** What would an implementer need to investigate?
- **Spot scope risks:** Is this actually three things disguised as one? Flag it.
- **Surface connections:** "This one and [other idea] seem related — should they be one item, or separate with a dependency?"

Don't do full shaping here — that's what /shape is for. Just enough structure that the items are useful when revisited later.

### Deduplication

Before filing, check against existing items from `bd list --json` results from setup.

If an idea overlaps with something existing, surface it: "This sounds similar to [existing item]. Should we merge them, keep them separate, or skip this one?"

Don't block filing over minor overlaps — just mention them.

---

## Filing

When the user is drained or you're approaching ~100K tokens of context, file everything.

### Summarize First

Present the full list of items you're about to file. Group them logically. For each item, show:
- Title
- One-line description
- Any flags (epic, overlaps with existing item, unclear scope)

**Get confirmation before filing.**

Create beads using `bd create` for each item:

```bash
bd create "Title of work item" \
  --description="One-line description of what this solves.

## Hill: Pending

## Context
Why this matters and what prompted it.

## Open Questions
- Things that need investigation during shaping" \
  -t task -p 2 \
  --labels human
```

- Default type: `task` (adjust if clearly a `bug`, `feature`, `refactor`, etc.)
- Default priority: `2` (adjust based on user's emphasis during extraction)
- Always include `## Hill: Pending` in the description — these are raw ideas that need shaping
- Always add `human` label — these items need /shape before Ralph can work on them
- For epics: create the epic bead noting it will need children, but don't prescribe what children should be

### Session End

After filing, present a summary:
- How many items filed
- List of titles with their IDs
- Any items flagged as epics or needing special attention
- Suggest running /shape on the most important items next

---

## Questioning Style

- **One or two questions at a time.** Don't barrage.
- **React to what they said.** Your next question should clearly follow from their last answer.
- **Challenge gently.** "Is that really one thing, or two?" / "What if you didn't build that?"
- **Use their language.** Mirror their terminology, don't impose your own.
- **Read the energy.** If they're on a roll, ask short prompts to keep momentum. If they're stuck, ask a provocative question to unlock new directions.
- **Never sound like a template.** Every question should feel handcrafted for this conversation.

---

## Start

1. Run `bd list --json` to see existing beads
2. Ask: **"What's on your mind?"**
"#;

/// Shape skill (with YAML frontmatter).
pub const SHAPE_SKILL_MD: &str = r#"---
name: shape
description: "Deep shaping session for work items. Use when the user wants to shape, refine, specify, or detail a work item, bead, or spec using Shape Up methodology."
---

# Shape

Drive work items up the hill from raw idea to implementation-ready spec. The hill model makes unknowns explicit: a bead starts with everything unknown (Pending), climbs as unknowns are resolved (Climbing), and reaches the top when nothing unknown remains (Shaped). Your job is to find unknowns and resolve them.

This is a sieve — each session is one refinement pass. You don't need to get from Pending to Shaped in one sitting. Move the bead as far as you can, record what's left, and come back.

## Setup

Run `bd list --json` to see existing beads for context.

---

## Entry Points

Support all three ways to start a shaping session:

### 1. Standalone

The user specifies a bead ID directly (e.g., "shape ralph-a12").

Run `bd show <id>` to load the item.

### 2. Continuation

The user says something like "shape the beads I just dumped" or "shape what we just captured."

Run `bd list --json --labels human` to find items flagged for human attention.

Present the list and let the user pick which to shape, or shape them all if they're related.

### 3. Auto-discover

If no item is specified, query for items that need shaping:

Run `bd list --json --labels human`.

Present the list and ask which item(s) to shape. If only one exists, offer to start with it.

---

## Hill Assessment (on entry)

Before anything else, read the bead and assess its hill position:

1. Check for a `## Hill` section in the description
2. Determine current position:
   - **Absent** → treat as Pending. Everything is unknown.
   - **Pending** → raw idea. Everything is unknown.
   - **Climbing** → partially shaped. Read the rationale for remaining unknowns.
   - **Shaped** → already at the top. Acknowledge and ask if the user wants to re-examine. If yes, start a fresh assessment — unknowns may have emerged since it was last shaped.

Present the assessment: *"This bead is at [position]. Here's what I see as unknown: [list]. Let me dig into the codebase before we go deeper."*

---

## Unknowns Taxonomy

Actively hunt for unknowns in these five categories. Not every category applies to every bead — use your judgment.

### Problem unknowns — do we understand WHY?
- What problem does this solve?
- Who has this problem?
- What would "done" look like from the user's perspective?

### Solution unknowns — do we know HOW?
- What's the approach? What are the alternatives?
- What key decisions need to be made?
- What trade-offs are we accepting?

### Codebase unknowns — do we know WHERE?
- Which files, functions, and patterns are involved?
- What existing code can be extended?
- What will break or need to change?
- Use subagents to research — don't ask about things you can discover.

### Edge case unknowns — do we know WHAT COULD GO WRONG?
- What inputs could be invalid?
- What external calls can fail?
- What state could be inconsistent?
- What happens under concurrent access or race conditions?
- For each failure mode, push for specific behavior: "What does the user see? Retry, fail gracefully, or surface the error?" Don't accept hand-waving.

### Scope unknowns — do we know WHAT'S OUT?
- What are we deliberately not doing?
- Where does this end and something else begin?
- What's the appetite — how much complexity is acceptable?

---

## Shaping Process

### Research the Codebase

**Before asking questions, investigate.** Use subagents in parallel to:

- Search for related code and existing patterns
- Find code that will be affected by this change
- Identify architectural conventions to follow
- Look for relevant tests
- Check for existing implementations that could be extended

**Bring findings to the user.** Research resolves codebase unknowns directly. Report what you found and mark those unknowns resolved.

### Track Unknowns Explicitly

Maintain a running list of unknowns. Present it periodically:

**Resolved:**
- ~~Problem: What problem does this solve?~~ → [answer]
- ~~Codebase: Which files are involved?~~ → [specific files and line numbers]

**Open:**
- Edge case: What happens when the network call fails?
- Scope: Should we support batch operations?
- Solution: Adapter pattern vs direct implementation — which fits?

As the conversation progresses:
- Mark unknowns resolved when answers emerge
- Add NEW unknowns when answers reveal them — this is normal and expected
- Be aggressive about finding unknowns. Don't stop at the surface.

### Resolve Through Conversation

Each question you ask should target a specific unknown. Drive questions from the unknowns list, not from a generic template.

When the problem warrants it, reach for these tools — but only when they serve resolving a specific unknown:

- **Spikes** — when there's genuine uncertainty about whether something will *work* (not just "how" but "if"). Use subagents to investigate. Report findings. Resolve uncertainty before committing to an approach.
- **Breadboarding** — when designing how components interact. Map affordances (what the user/system can do) and wiring (how components connect). Use when interactions are non-obvious.
- **Shape alternatives** — when multiple approaches exist and the trade-offs aren't clear. Compare against requirements. Use fit checks to make the decision concrete.

### Vertical Slicing

When the solution shape is clear, break it into the smallest valuable increments:

- Cuts through all layers (not "build API, then UI" — one thin feature end-to-end)
- Delivers something observable
- Works independently, even if limited
- Is smaller than you think

**If a slice has "and" in it, it's probably two slices.** Break it down.

**Challenge aggressively:**
- "What if we didn't include X in this slice?"
- "Can we ship just the happy path first?"
- "What's the smallest thing that would be valuable?"

**Red flags:**
- "Build the infrastructure for X" → No user value. Combine with first use.
- "This sets up Y for later" → Do Y now as a thin slice instead.
- "It's all one thing" → What about happy path only?

### Dependencies

Check for cross-bead dependencies (`bd list --json`) and propose an order for slices. If slices depend on each other, set dependencies with `bd dep add`.

---

## What Makes a Bead Shaped (top of the hill)

A bead is Shaped when an agent can execute it without asking clarifying questions:

- **Problem** is understood — why are we doing this?
- **Approach** is grounded in the codebase — specific files, line numbers, patterns to follow
- **Edge cases** are identified — error conditions, boundary behavior
- **Acceptance criteria** are specific and testable
- **Test plan** exists — what to test, key scenarios
- **Scope** is clear — what's in, what's out
- **No open unknowns remain**

If ANY unknown is still open, the bead is Climbing, not Shaped. Be honest about this.

---

## Output (on exit)

When the conversation converges — or when a pass is done — persist the shaping artifacts.

### Summarize First

Present the full shaped output and confirm with the user before writing. Include the hill assessment, resolved unknowns, and any remaining unknowns.

### Set Hill Status

Update the bead description with all shaping artifacts and the hill status:

**If unknowns remain** (Climbing):
```bash
bd update <id> --description="$(cat <<'EOF'
[existing shaped content — approach, edge cases, acceptance, etc.]

## Hill: Climbing
Remaining unknowns:
- [category]: [what's still unknown]
- [category]: [what's still unknown]
EOF
)"
```
Keep the `human` label — the bead needs more shaping.

**If all unknowns are resolved** (Shaped):
```bash
bd update <id> --description="$(cat <<'EOF'
[full shaped content — approach, edge cases, acceptance, tests, etc.]

## Hill: Shaped
EOF
)"
```
Remove the `human` label — the bead is ready for Ralph:
```bash
bd update <id> --remove-label=human
```

### Description Structure

Include these sections in the shaped description (as appropriate for the bead):

- **## What done looks like** — observable outcome from user's perspective
- **## Approach** — chosen solution, grounded in codebase (files, line numbers, patterns)
- **## Edge Cases** — failure modes and specific behaviors
- **## Acceptance** — specific, testable criteria
- **## Tests** — what to test, key scenarios
- **## Hill: [status]** — always last, with rationale if Climbing

If `bd update` fails, surface the error and don't lose the shaping work — present the content to the user so they can capture it.

---

## Shaping Multiple Related Items

When shaping multiple items in one session:
- Consider how they inform each other's scope and boundaries
- Look for shared unknowns or overlapping concerns
- Identify dependencies between items
- Shape them as a cohesive set, not independently
- Each gets its own independent hill assessment

---

## Error Handling

- **Bead not found:** Surface clearly, offer to list available items
- **bd update failures:** Surface the error, present the shaped content to the user so nothing is lost
- **Already Shaped:** Acknowledge the bead is at the top of the hill. Ask if the user wants to re-examine — unknowns may have emerged since it was last shaped. If yes, start fresh.
- **User disagrees with hill assessment:** User's judgment overrides. Adjust.
- **User wants to mark Shaped with open unknowns:** Warn that open unknowns mean an agent may get stuck, but respect the user's decision.

---

## Interview Style

- Ask one or two questions at a time, not a barrage
- **Research first, then ask informed questions**
- Bring information to the user — don't make them guess
- Reflect back what you heard to confirm understanding
- Push back when scope seems too broad or slices too big
- Let the conversation zoom in and out naturally
- **Every question should be creative and context-specific.** Drive questions from what the user actually said and from the unknowns list. Never fall back on generic templates. Each question should feel like it came from a thinking partner who's been paying attention.

---

## Start

1. Run `bd list --json` to see existing beads
2. Determine entry point:
   - If the user specified an item → load it (standalone)
   - If the user mentioned recent items → find `human`-labeled items (continuation)
   - If neither → query for `human`-labeled items and present the list (auto-discover)
3. Assess hill position — read the bead, check for `## Hill` section, identify unknowns
4. Present the assessment, then begin the shaping conversation
"#;

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
    fn brain_dump_skill_includes_hill_pending() {
        assert!(
            BRAIN_DUMP_SKILL_MD.contains("## Hill: Pending"),
            "brain dump bead template should include Hill: Pending"
        );
    }

    #[test]
    fn brain_dump_skill_uses_human_label() {
        assert!(
            BRAIN_DUMP_SKILL_MD.contains("--labels human"),
            "brain dump should label beads with human, not needs-shaping"
        );
    }

    #[test]
    fn shape_skill_contains_hill_assessment() {
        assert!(
            SHAPE_SKILL_MD.contains("Hill Assessment"),
            "shape skill should assess hill position on entry"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Pending"),
            "shape skill should recognize Pending hill position"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Climbing"),
            "shape skill should recognize Climbing hill position"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Shaped"),
            "shape skill should recognize Shaped hill position"
        );
    }

    #[test]
    fn shape_skill_contains_unknowns_taxonomy() {
        assert!(
            SHAPE_SKILL_MD.contains("Problem unknowns"),
            "shape skill should include problem unknowns category"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Solution unknowns"),
            "shape skill should include solution unknowns category"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Codebase unknowns"),
            "shape skill should include codebase unknowns category"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Edge case unknowns"),
            "shape skill should include edge case unknowns category"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Scope unknowns"),
            "shape skill should include scope unknowns category"
        );
    }

    #[test]
    fn shape_skill_defines_shaped_criteria() {
        assert!(
            SHAPE_SKILL_MD.contains("What Makes a Bead Shaped"),
            "shape skill should define what makes a bead Shaped"
        );
        assert!(
            SHAPE_SKILL_MD.contains("No open unknowns remain"),
            "shape skill should require no open unknowns for Shaped status"
        );
    }

    #[test]
    fn shape_skill_sets_hill_status_on_exit() {
        assert!(
            SHAPE_SKILL_MD.contains("## Hill: Climbing"),
            "shape skill should instruct setting Climbing status"
        );
        assert!(
            SHAPE_SKILL_MD.contains("## Hill: Shaped"),
            "shape skill should instruct setting Shaped status"
        );
    }

    #[test]
    fn shape_skill_manages_human_label() {
        assert!(
            SHAPE_SKILL_MD.contains("--remove-label=human"),
            "shape skill should remove human label when Shaped"
        );
        assert!(
            SHAPE_SKILL_MD.contains("Keep the `human` label"),
            "shape skill should keep human label when Climbing"
        );
    }
}
