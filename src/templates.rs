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

/// Specs README template with status key and empty table.
pub const SPECS_README_MD: &str = r#"# Specs

Single source of truth for specification status.

## Status Key

- **Ready** — Shaped and ready for implementation
- **In Progress** — Currently being worked on
- **Done** — Complete
- **Blocked** — Cannot proceed (see spec for details)

## Specs

| Spec | Status | Summary | Depends On |
|------|--------|---------|------------|

## Notes

When picking work:
- Choose specs marked **Ready**
- Respect dependencies (don't start until dependencies are Done)
"#;

/// Spec template with authoring guidelines.
pub const SPECS_TEMPLATE_MD: &str = r#"# Spec Guidelines

These are guidelines, not a rigid template. Shape each spec to fit its content.

## What Every Spec Needs

**User-facing behavior** — What can the user do when this is complete? Be concrete.

**Acceptance criteria** — Checkboxes that define done. For multi-slice specs, group criteria by slice.

**Technical constraints** — Architecture decisions, patterns to follow, things to avoid.

**Error cases** — How should the system behave when things go wrong? Be comprehensive.

## Optional Sections

**Out of scope** — Include when boundaries might be unclear to prevent scope creep.

**Dependencies** — If this spec depends on another spec being complete.

## Slices

A slice is the smallest unit of work that delivers user value. When a spec has multiple slices:

- Each slice should be completable in one agent session
- Group acceptance criteria under slice headings
- Slices can have dependencies on each other
- Prefer slices that are **core** (central to the concept), **small** (completable quickly), and **novel** (reduce uncertainty)

## Example Structure

```markdown
# Feature Name

One-line description of what the user can do.

## Slice 1: [Name]

### User Behavior
What the user experiences.

### Acceptance Criteria
- [ ] Criterion one
- [ ] Criterion two

### Technical Constraints
How to build it.

### Error Cases
- When X happens, show Y
- When Z fails, do W

## Slice 2: [Name]
(depends on Slice 1)

...
```

## Living Document

Specs evolve. Agents may update specs as they learn—adding discovered requirements, refining criteria, or documenting blockers.
"#;

/// Project-specific config file template.
pub const RALPH_CONFIG: &str =
    "# Project-specific Ralph config \u{2014} edit with config modal (c)\n";

/// Specs-mode work discovery instructions (compiled into binary).
pub const SPECS_MODE_MD: &str = r#"
# Specs Mode

## Work Discovery

Read `specs/README.md` to find specs marked **Ready**.

**To flag:** Note issues in the spec file or README.

**To mark in progress:** Update status to **In Progress** in `specs/README.md` and commit.

## Details

Read the spec file for acceptance criteria, technical constraints, and error cases.

## Progress Tracking

- Mark completed items in the spec with `[x]`
- Keep `specs/README.md` status accurate

## When Blocked

Document in BOTH the spec AND `specs/README.md`.

## Completing Work

Mark the spec complete in `specs/README.md`.
"#;

/// Beads-mode work discovery instructions (compiled into binary).
pub const BEADS_MODE_MD: &str = r#"
# Beads Mode

## Work Discovery

Run `bd ready --json` to find available work.

**To flag:** `bd label add <id> shaping-required` or `bd label add <id> too-big`

**To mark in progress:** `bd update <id> --status in_progress` (one bead at a time)

## Details

Run `bd show <id>` for full bead details.

## When Blocked

```
bd comments add <id> -m "What failed, why it blocks, options to resolve"
bd label add <id> blocked
```

## Completing Work

Close the bead: `bd close <id>`
"#;

/// Returns mode-specific prompt content for the given mode, if any.
pub fn mode_content(mode: &str) -> Option<&'static str> {
    match mode {
        "specs" => Some(SPECS_MODE_MD),
        "beads" => Some(BEADS_MODE_MD),
        _ => None,
    }
}

/// Spec shaping interview command.
pub const RALPH_SPEC_MD: &str = r#"# Spec Shaping Interview

Shape a specification through conversation. Your goal: produce a spec that an implementing agent can execute in small, valuable vertical slices.

## Setup

Read `specs/README.md` and `specs/TEMPLATE.md` before starting.

---

## Phase 1: What Are We Working On?

Ask: **"What are we working on?"**

Classify the work type—this shapes the entire interview:

| Type | Value Statement | Key Questions |
|------|-----------------|---------------|
| **Feature** | "User can now do X" | What can they do when it's done? Why does it matter? |
| **Bug fix** | "X no longer happens" | How do we reproduce it? What's expected vs actual? |
| **Refactor** | "Code is now Y" | What's painful now? What's the target state? |
| **Performance** | "X is N% faster" | What's slow? Current vs target metrics? |
| **Security** | "Vulnerability X is fixed" | What's the risk? What's the remediation? |
| **Tech debt** | "We can now do Y more easily" | What's the pain? Cost of inaction? |

---

## Phase 2: Research the Codebase

**Before asking more questions, research.** Use subagents in parallel to:

- Search for related code and existing patterns
- Find code that will be affected by this change
- Identify architectural conventions
- Look for relevant tests
- For bugs: find the code path and existing error handling

**Bring findings to the user:** "I found [X] in the codebase. Here's what I see..."

This grounds the conversation in reality. Don't ask "what patterns should we follow?" when you can find them.

---

## Phase 3: Problem & Value

Now ask informed questions based on your research.

**For features:**
- "When this is done, what can the user DO that they couldn't before?"
- "Walk me through exactly what the user sees and does."
- "Why does this matter?"

**For bugs:**
- "What's the expected behavior vs actual behavior?"
- "Here's what I found in [code area]. Does this look like the problem?"
- "What's the user impact?"

**For refactors:**
- "What's painful about the current implementation?"
- "What's the target state?"
- "How do we migrate safely?"

**For performance:**
- "What's the current performance? What's the target?"
- "I found [potential bottleneck]. Is this the area?"

Keep asking until you can articulate the value in one sentence.

---

## Phase 4: Vertical Slices

This is critical. Break work into **vertical slices**—each slice delivers observable value.

### What Makes a Good Slice

A vertical slice:
- Cuts through all layers (not "build API, then UI"—build one thin feature end-to-end)
- Delivers something the user can see, verify, or benefit from
- Works independently, even if limited

### Shape Up Criteria

Use these to prioritize slices:

**Core:** Is this central to the concept?
- "Without this, the other work wouldn't mean anything"
- Do core slices first

**Small:** Can this be completed in one agent session?
- If not, slice thinner
- A few days of work, not weeks

**Novel:** Does this reduce uncertainty?
- Unproven approaches should be validated early
- "We've never done X before" → do X first

### Slicing Questions

For each potential slice:
- "Can the user verify this works independently?"
- "Is this the smallest useful increment?"
- "Does this depend on another slice being done first?"

### Challenge Aggressively

Push for smaller slices:
- "What if we didn't include X in this slice?"
- "Can we ship just the happy path first?"
- "What's the smallest thing a user would notice?"

Keep slicing until the user pushes back with a good reason.

### Red Flags

Challenge these:
- "Build the infrastructure for X" → No user value yet. Combine with first use.
- "This sets up Y for later" → Do Y now as a thin slice instead.
- "It's all one thing, can't be split" → What about happy path only?
- "We need to refactor before we can build" → Can we do the smallest refactor + smallest feature together?

---

## Phase 5: Technical Constraints

**Research with subagents**, then present findings:

- "Looking at [similar code], I see this pattern. Should we follow it?"
- "I found [utility/helper]. We should reuse this."
- "The codebase uses [convention] for error handling. We should match."
- "This will touch [files]. Here's the current structure."

Present constraints—don't just ask "what are the constraints?"

---

## Phase 6: Error Cases

Be comprehensive. Use your research to identify failure modes:

- What external calls can fail? (network, filesystem, processes)
- What inputs could be invalid?
- What state could be inconsistent?
- What happens under resource pressure?

For each failure mode:
- "How should the system behave when X fails?"
- "What does the user see?"
- "Should we retry, fail gracefully, or surface the error?"

**Don't accept hand-waving.** Push for specific behaviors.

---

## Phase 7: Dependencies

**Check with subagents:**
- Review `specs/README.md` for dependencies on other specs
- Identify which slices depend on other slices
- Check if required code/features already exist

**Propose an order:** "Based on dependencies, I suggest: Slice 1 → Slice 2 → Slice 3. Does this make sense?"

---

## Phase 8: Boundaries

Only if scope is ambiguous:
- "What might someone assume is included that isn't?"
- "What related work are we explicitly deferring?"

---

## Phase 9: Write the Spec

When the conversation converges:

1. **Summarize** what you're about to write and confirm with the user
2. **Create** `specs/[name].md` following `specs/TEMPLATE.md`
3. **Update** `specs/README.md`:
   - Add to the table
   - Set status to **Ready**
   - Write one-line summary
   - List dependencies

## Phase 10: Commit

Make a commit once getting approval from the user

---

## Interview Style

- Ask one or two questions at a time, not a barrage
- **Research first, then ask informed questions**
- Bring information to the user—don't make them guess
- Reflect back what you heard to confirm understanding
- Push back when slices seem too big
- Let the conversation zoom in and out naturally

---

## Start

Read `specs/README.md` and `specs/TEMPLATE.md`, then ask: **"What are we working on?"**
"#;
