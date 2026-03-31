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

- **Needs Shaping** — Captured but not yet shaped — needs refinement via /shape
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

/// Generate beads-mode prompt content based on claim state.
/// Command reference is provided by `bd prime` — this only covers Ralph-specific workflow.
pub fn beads_mode_content(claimed_bead_id: Option<&str>) -> String {
    let work_section = match claimed_bead_id {
        Some(id) => format!(
            r#"## Your Work

Ralph claimed bead `{id}` for you. Run `bd show {id}` to read its full specification.

### Assess Before Building

Research the codebase and the bead before committing to build it.

**Build it** if the bead is well-specified and this is the right work.

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

/// Returns mode-specific prompt content for the given mode, if any.
pub fn mode_content(mode: &str, claimed_bead_id: Option<&str>) -> Option<String> {
    match mode {
        "specs" => Some(SPECS_MODE_MD.to_string()),
        "beads" => Some(beads_mode_content(claimed_bead_id)),
        _ => None,
    }
}

/// Brain dump skill (with YAML frontmatter).
pub const BRAIN_DUMP_SKILL_MD: &str = r#"---
name: brain-dump
description: "Intense idea extraction session. Use when the user wants to brain dump, dump thoughts, capture ideas, process voice notes, raw notes, or stream of consciousness into structured work items."
---

# Brain Dump

Drain ideas from the user's head through intense, relentless questioning, then file them as rough work items. Most items will need further shaping via /shape.

## Setup

**Detect mode** by checking the project directory. If a `.beads/` directory exists, it's beads mode. Otherwise, specs mode (default).

- **Beads mode**: Run `bd list --json` to see existing beads for deduplication context.
- **Specs mode** (default): Read `specs/README.md` to see existing specs for deduplication context.

**Announce the mode:** Tell the user which mode you detected. Example: *"I see this project uses beads mode, so I'll file ideas as beads. Let me know if you'd prefer specs instead."*

If the user wants to override the detected mode, respect their choice.

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

Before filing, check against existing items:
- **Beads mode:** Compare against `bd list --json` results from setup
- **Specs mode:** Compare against `specs/README.md` entries

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

### Beads Mode

Create beads using `bd create` for each item:

```bash
bd create "Title of work item" \
  --description="One-line description of what this solves.

## Context
Why this matters and what prompted it.

## Open Questions
- Things that need investigation during shaping" \
  -t task -p 2 \
  --labels needs-shaping
```

- Default type: `task` (adjust if clearly a `bug`, `feature`, `refactor`, etc.)
- Default priority: `2` (adjust based on user's emphasis during extraction)
- Always add `needs-shaping` label — these items need /shape before implementation
- For epics: create the epic bead noting it will need children, but don't prescribe what children should be

### Specs Mode

Create rough spec files for each item:

1. **Create** `specs/[name].md` with rough content:

```markdown
# Item Title

One-line description of what this solves.

## Context
Why this matters and what prompted it.

## Open Questions
- Things that need investigation during shaping
```

2. **Update** `specs/README.md`:
   - Add each spec to the table
   - Set status to **Needs Shaping**
   - Write one-line summary

### Session End

After filing, present a summary:
- How many items filed
- List of titles with their IDs (beads mode) or filenames (specs mode)
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

1. Detect mode (`.beads/` dir → beads, otherwise → specs)
2. **Beads mode:** Run `bd list --json` to see existing beads
3. **Specs mode:** Read `specs/README.md` to see existing specs
4. Announce the detected mode to the user
5. Ask: **"What's on your mind?"**
"#;

/// Shape skill (with YAML frontmatter).
pub const SHAPE_SKILL_MD: &str = r#"---
name: shape
description: "Deep shaping session for work items. Use when the user wants to shape, refine, specify, or detail a work item, bead, or spec using Shape Up methodology."
---

# Shape

Deeply refine rough work items into fully shaped, implementation-ready specifications using Shape Up methodology. Your goal: take a vague or under-specified item and produce something an implementing agent can execute in small, valuable vertical slices.

## Setup

**Detect mode** by checking the project directory. If a `.beads/` directory exists, it's beads mode. Otherwise, specs mode (default).

- **Beads mode**: Run `bd list --json` to see existing beads for context.
- **Specs mode** (default): Read `specs/README.md` and `specs/TEMPLATE.md` before starting.

**Announce the mode:** Tell the user which mode you detected. Example: *"I see this project uses beads mode, so I'll shape beads in place. Let me know if you'd prefer specs instead."*

If the user wants to override the detected mode, respect their choice.

---

## Entry Points

Support all three ways to start a shaping session:

### 1. Standalone

The user specifies a bead ID or spec name directly (e.g., "shape ralph-a12" or "shape the auth spec").

- **Beads mode:** Run `bd show <id>` to load the item
- **Specs mode:** Read `specs/<name>.md` to load the item

### 2. Continuation

The user says something like "shape the beads I just dumped" or "shape what we just captured."

- **Beads mode:** Run `bd list --json --labels needs-shaping` to find recent `needs-shaping` items
- **Specs mode:** Read `specs/README.md` and look for items with "Needs Shaping" status

Present the list and let the user pick which to shape, or shape them all if they're related.

### 3. Auto-discover

If no item is specified, query for items that need shaping:

- **Beads mode:** Run `bd list --json --labels needs-shaping`
- **Specs mode:** Read `specs/README.md` and find "Needs Shaping" entries

Present the list and ask which item(s) to shape. If only one exists, offer to start with it.

---

## Shaping Process

### Phase 1: Understand the Raw Item

Read the existing description or spec content. Identify:
- What's clear vs. vague
- What problem this solves
- Who benefits and why

Then tell the user what you understand so far: *"Here's what I see in this item — [summary]. Let me dig into the codebase before we go deeper."*

### Phase 2: Research the Codebase

**Before asking questions, investigate.** Use subagents in parallel to:

- Search for related code and existing patterns
- Find code that will be affected by this change
- Identify architectural conventions to follow
- Look for relevant tests
- Check for existing implementations that could be extended

**Bring findings to the user:** "I found [X] in the codebase. Here's what I see..."

Ground the conversation in reality. Don't ask about patterns you can discover.

### Phase 3: Requirements Tracking

Build requirements as they emerge through conversation. Track each with an ID and status:

| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Core goal — what this delivers | core goal |
| R1 | Must support X | must-have |
| R2 | Should handle Y | nice-to-have |
| R3 | Edge case Z | undecided |
| R4 | Feature W | out |

**Statuses:** core goal, must-have, nice-to-have, undecided, out

Surface the R table periodically so the user can see what's accumulating and adjust priorities. Ask about undecided items: *"R3 — you mentioned edge case Z. Is that must-have for this appetite, or can we cut it?"*

Requirements emerge through conversation, not interrogation. Listen for implicit requirements in the user's answers and surface them: *"It sounds like R4 — users need feedback when X happens. Adding that as must-have. Agree?"*

### Phase 4: Shape Alternatives

When the problem warrants it, explore multiple solution approaches:

| Shape | Approach | Trade-offs |
|-------|----------|------------|
| A | Direct implementation | Simple but doesn't scale |
| B | Adapter pattern | More complex, handles future cases |
| C | Event-driven | Most flexible, highest complexity |

Not every item needs multiple shapes. Use your judgment:
- Clear, small items → one obvious shape, move on
- Ambiguous or large items → explore 2-3 shapes before committing

For each shape, note the key trade-offs. Don't overanalyze — the goal is to find the right approach, not enumerate every option.

### Phase 5: Fit Checks

When comparing shapes against requirements, use a binary decision matrix:

| | R0 | R1 | R2 | R3 |
|---|---|---|---|---|
| Shape A | ✅ | ✅ | ❌ | ❌ |
| Shape B | ✅ | ✅ | ✅ | ❌ |
| Shape C | ✅ | ✅ | ✅ | ✅ |

This makes the trade-offs concrete: *"Shape B covers all must-haves but drops R3. Shape C covers everything but adds complexity. Given our appetite, which fits?"*

Skip fit checks when there's only one viable shape.

### Phase 6: Spikes

When there's genuine uncertainty about whether something will work — not just "how" but "if" — run a spike:

1. Identify what's uncertain: *"I'm not sure if the event system can handle this pattern. Let me check."*
2. Use subagents to investigate the codebase
3. Report findings: *"I looked at the event system — it uses [pattern]. This means Shape B would work if we [X], but Shape C would require [Y]."*

Spikes resolve uncertainty before committing to a shape. Don't spike everything — only genuinely uncertain mechanics.

### Phase 7: Breadboarding

When designing system interactions (how components talk to each other, user flows, data flow), map the affordances and wiring:

**Affordances** — what the user/system can do at each point:
- Screen/view shows [X]
- User can [action]
- System responds with [Y]

**Wiring** — how components connect:
- Component A calls Component B
- Event X triggers Handler Y
- Data flows from Source → Transform → Destination

Use breadboarding when the interactions are non-obvious. Skip it for straightforward CRUD or simple changes.

### Phase 8: Vertical Slicing

Break the shaped work into the smallest valuable increments.

**What makes a good slice:**
- Cuts through all layers (not "build API, then UI" — build one thin feature end-to-end)
- Delivers something the user can see, verify, or benefit from
- Works independently, even if limited
- Is smaller than you think — one focused change per slice

**If a slice has "and" in it, it's probably too big.** Break it down.

**Shape Up criteria for prioritizing slices:**

- **Core:** Central to the concept? "Without this, the other work wouldn't mean anything." Do core slices first.
- **Small:** Completable in one short agent session? If not, slice thinner.
- **Novel:** Reduces uncertainty? Unproven approaches should be validated early.

**Challenge aggressively:**
- "What if we didn't include X in this slice?"
- "Can we ship just the happy path first?"
- "What's the smallest thing a user would notice?"

**Red flags to challenge:**
- "Build the infrastructure for X" → No user value yet. Combine with first use.
- "This sets up Y for later" → Do Y now as a thin slice instead.
- "It's all one thing, can't be split" → What about happy path only?

### Phase 9: Error Cases

Be comprehensive. Use your research to identify failure modes:

- What external calls can fail? (network, filesystem, processes)
- What inputs could be invalid?
- What state could be inconsistent?

For each failure mode, push for specific behaviors:
- "How should the system behave when X fails?"
- "What does the user see?"
- "Should we retry, fail gracefully, or surface the error?"

Don't accept hand-waving. Push for concrete answers.

### Phase 10: Dependencies

**Check with subagents:**
- **Beads mode:** Run `bd list --json` to check for dependencies on existing beads
- **Specs mode:** Review `specs/README.md` for dependencies on other specs
- Identify which slices depend on other slices

**Propose an order:** "Based on dependencies, I suggest: Slice 1 → Slice 2 → Slice 3. Does this make sense?"

---

## Output

When the conversation converges, persist the shaping artifacts.

### Summarize First

Present the full shaped output and confirm with the user before writing:
- Requirements table (R0-Rn with statuses)
- Chosen shape and rationale
- Vertical slices in order
- Error handling decisions
- Dependencies

### Beads Mode

Update the bead in place using `bd update <id>` to enrich the description with shaping artifacts:

```bash
bd update <id> --description="$(cat <<'EOF'
## Value
[One sentence: what this delivers and why it matters]

## Requirements
| ID | Requirement | Status |
|----|-------------|--------|
| R0 | Core goal | core goal |
| R1 | ... | must-have |

## Shape Decision
[Chosen approach and why]

## Slices
- [ ] Slice 1: [description] (core)
- [ ] Slice 2: [description]
- [ ] Slice 3: [description]

## Error Cases
- [failure mode]: [specific behavior]

## Technical Constraints
- [constraint from codebase research]
EOF
)"
```

After updating, remove the `needs-shaping` label:
```bash
bd label remove <id> needs-shaping
```

If `bd update` fails, surface the error and don't lose the shaping work — present the content to the user so they can capture it.

### Specs Mode

Enrich the spec file content at `specs/<name>.md` with the shaping artifacts.

Update `specs/README.md`:
- Change status from "Needs Shaping" to **Ready**

Commit the changes after getting user approval.

---

## Shaping Multiple Related Items

When shaping multiple items in one session:
- Consider how they inform each other's scope and boundaries
- Look for shared requirements or overlapping slices
- Identify dependencies between items
- Shape them as a cohesive set, not independently

---

## Error Handling

- **Bead/spec not found:** Surface clearly, offer to list available items
- **bd update failures:** Surface the error, present the shaped content to the user so nothing is lost
- **Already-shaped items:** Note that the item appears already shaped, ask if the user wants to reshape it

---

## Interview Style

- Ask one or two questions at a time, not a barrage
- **Research first, then ask informed questions**
- Bring information to the user — don't make them guess
- Reflect back what you heard to confirm understanding
- Push back when scope seems too broad or slices too big
- Let the conversation zoom in and out naturally
- **Every question should be creative and context-specific.** Drive questions from what the user actually said. Never fall back on generic templates. Each question should feel like it came from a thinking partner who's been paying attention.

---

## Start

1. Detect mode (`.beads/` dir → beads, otherwise → specs)
2. **Beads mode:** Run `bd list --json` to see existing beads
3. **Specs mode:** Read `specs/README.md` and `specs/TEMPLATE.md`
4. Determine entry point:
   - If the user specified an item → load it (standalone)
   - If the user mentioned recent items → find `needs-shaping` items (continuation)
   - If neither → query for `needs-shaping` items and present the list (auto-discover)
5. Announce the detected mode
6. Summarize what you see in the item, then begin the shaping conversation
"#;
