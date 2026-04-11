---
name: shape
description: "Deep shaping session for work items. Use when the user wants to shape, refine, specify, or detail a work item, bead, or spec using Shape Up methodology."
---

# Shape

Refine work items through conversation using Shape Up methodology. You are a **sieve pass operator** — each session does the *next appropriate refinement*, not all refinements at once. Raw ideas become epics (bounded contexts), epics get sliced into vertical scopes, slices get light direction added. Trust the sieve to handle multiple passes.

```
raw ideas, epics, beads
            │
            ▼
┌══════════════════════┐
│ ░░░ conversation ░░░ │──► new epics / beads (back to top)
├──────────┬───────────┘
           │ (passes through)
┌──────────┴───────────┐
│ ░░░ conversation ░░░ │──► new epics / beads (back to top)
├──────────┬───────────┘
           │
          ...
           │
           ▼
    ╔══════════════╗
    ║  RALPH LOOP  ║──► SOFTWARE
    ╚══════╤═══════╝
           │
           │ "not ready"
           │ (updates bead with what's missing)
           │
           └──► back into the sieve next session
```

Your job is to do **one pass** of this sieve. Put the right amount of content at the right level of detail on each artifact. Avoid over-specifying. Avoid under-specifying. Trust agents downstream to be smart.

---

## Setup

Run `bd list --json` to see existing beads for context.

---

## Entry Points

Support all three ways to start a shaping session:

### 1. Standalone

The user specifies a bead ID directly (e.g., "shape ralph-a12").

Run `bd show <id>` to load the item. If it's an epic, also run `bd children <id>` to see existing slices.

### 2. Continuation

The user says something like "shape the beads I just dumped" or "shape what we just captured."

Run `bd list --label=human` and `bd blocked` to find items needing refinement.

Present the list and let the user pick which to shape, or shape them all if they're related.

### 3. Auto-discover

If no item is specified, query for items that need shaping: run `bd list --label=human` and `bd blocked` to find items needing attention.

Present the list and ask which item(s) to shape. If only one exists, offer to start with it.

---

## Step 1: Read the Input

Load the item and assess where it is in its lifecycle:

- **Raw idea** — vague, maybe just a sentence or voice dump. Needs exploring, expanding, or refining before it can become a bounded context.
- **Partially refined** — has some shape but isn't yet a clear bounded context. Might need expanding, narrowing, clarifying, or some combination.
- **Bounded context (epic)** — clear problem, rough solution shape, defined boundaries and no-gos. Ready to discover vertical slices.
- **Epic with slices** — has child beads but some slices may need more direction or the epic itself may need adjustment.
- **Slice needing direction** — a vertical slice that needs light guidance about approach, relevant code, or constraints.

Don't force the input into a category. Read what's actually there.

---

## Step 2: Diagnose the Pass

Tell the user what kind of refinement you think this needs next and what the output will look like. Be specific about the artifacts:

*"This is a raw idea that needs exploring. Let's figure out what problem we're actually solving. Output will probably be an epic with clear boundaries, maybe some child beads if we get that far."*

*"This epic has good boundaries but no slices yet. Let's discover the vertical scopes. I'll create child beads for each one."*

*"This slice got kicked back — looks like it needs more direction about [specific thing]. Let's add that and get it ready."*

The user should know what pass they're in and what comes out the other end.

---

## Step 3: Run the Conversation

This is the core of the session. Use your judgment about which techniques to reach for based on what the input actually needs. These are **tools in your toolkit**, not a sequence to follow:

### Exploring & Expanding
For raw or vague ideas. Ask questions that help the user articulate what they're really after. What's the problem? Who has it? Why does it matter? What would "done" look like? Don't rush to bound — sometimes ideas need room to breathe first.

### Bounding
When the problem is understood but the scope isn't clear. Define what's in and what's out. Identify no-gos. Set the appetite. The output is a bounded context — an epic that tells builders where to play and where to stop.

### Codebase Research
When the conversation would benefit from grounding in reality. Use subagents to search for related code, existing patterns, affected areas, and conventions. Bring findings to the user — don't make them guess. Research before asking questions about things you can discover.

### Slicing
When a bounded context is ready to be broken into vertical scopes. Discover the natural seams. Each slice should cut through all layers, deliver something independently valuable, and be completable in one agent session.

**Good slices:**
- Cut through all layers (not "build API, then UI")
- Deliver something visible and verifiable
- Work independently, even if limited
- Are smaller than you think

**Challenge aggressively:**
- "What if we didn't include X in this slice?"
- "Can we ship just the happy path first?"
- If a slice has "and" in it, it's probably two slices

### Spikes
When there's genuine uncertainty about whether something will *work* (not just "how" but "if"). Use subagents to investigate. Report findings. Resolve uncertainty before committing to an approach.

### Breadboarding
When designing how components interact — user flows, data flow, system wiring. Map affordances (what the user/system can do) and wiring (how components connect). Use when interactions are non-obvious.

### Adding Direction
For slices that need guidance before autonomous execution. Point to relevant code, note constraints, suggest an approach. Include edge cases and error conditions — leaving these out makes the agent need to guess, which leads to bugs. Keep it lean but complete: an agent should be able to verify "done" without asking questions.

---

## Step 4: Produce Output

When the conversation converges, present what you plan to create and confirm with the user before writing. Show the artifacts, their content, and their labels/metadata.

### The Epic (Bounded Context)

Create or update an epic bead that holds:
- **What problem this solves** and why it matters
- **The rough shape of the solution** — enough for a builder to understand the approach without over-specifying
- **Boundaries and no-gos** — where to stop, what's explicitly out of scope

```bash
bd create --title="Epic title" --type=feature --description="$(cat <<'EOF'
## Problem
[What problem this solves and why it matters]

## Solution Shape
[Rough approach — enough to guide builders, not a spec]

## Boundaries
- [What's in scope]
- [No-gos — what's explicitly out]
EOF
)" --priority=2
```

After creating, flag for human so it stays in the shaping queue until fully shaped:

```bash
bd update <id> --add-label=human
```

If the epic already exists, use `bd update <id>` instead.

### Child Beads (Vertical Slices)

Child beads are what the autonomous loop actually executes. They must be self-contained — an agent working from a child bead cannot ask clarifying questions. If any of these sections have gaps, dig deep into the codebase to ground the spec in reality, and surface open questions to the user for their judgment.

Each child gets:
- A **scope name** as the title — this becomes the shared vocabulary for the project
- **What done looks like** — specific enough that an agent can verify completion
- **Approach** — which files to modify, which patterns to follow, key decisions
- **Edge cases** — error conditions, boundary behavior, what could go wrong
- **Acceptance criteria** — at least one specific, testable condition
- **Tests** — new behavior needs test coverage. See `TESTING.md` for the project's testing philosophy. Only omit tests when the behavior is genuinely impractical to test (e.g., terminal rendering, signal handling); that should be the exception, not the default

```bash
bd create --title="Scope name" --type=task --parent=<epic-id> --description="$(cat <<'EOF'
[What done looks like for this slice]

## Approach
[Files to modify, patterns to follow, key decisions]

## Edge Cases
[Error conditions, boundary behavior, what could go wrong]

## Acceptance
- [Specific, testable condition]

## Tests
[What to test, key scenarios. Omit only when genuinely impractical.]
EOF
)" --priority=2
```

After creating each child, flag for human if it still needs another shaping pass:

```bash
bd update <id> --add-label=human
```

### Readiness

- Flag items for human (`bd update <id> --add-label=human`) when they need another pass through the sieve
- Unflag items from human (`bd update <id> --remove-label=human`) when they're ready for the Ralph Loop
- Set appropriate priority, type, and any other relevant metadata
- Set dependencies between child beads when order matters: `bd dep add <child> <depends-on>`

### If `bd` commands fail

Surface the error immediately. Present all the shaped content to the user so nothing is lost. Don't silently retry.

### Partial Passes

Not every session produces slices. A valid output might be:
- Just an epic with clearer boundaries (raw idea → bounded context)
- An updated epic with adjusted scope (re-bounding after feedback)
- A single slice with more direction (adding guidance to a kicked-back bead)
- Multiple new epics discovered from one big idea (splitting)

Do the next refinement. The sieve handles the rest.

---

## Shaping Multiple Related Items

When shaping multiple items in one session:
- Consider how they inform each other's scope and boundaries
- Look for shared context or overlapping slices
- Identify dependencies between items
- Shape them as a cohesive set, not independently

---

## Error Handling

- **Bead not found:** Surface clearly, offer to list available items
- **bd command failures:** Surface the error, present the shaped content to the user so nothing is lost
- **Already-shaped items:** Note that the item appears already shaped, ask if the user wants to reshape it

---

## Interview Style

- Ask one or two questions at a time, not a barrage
- **Research first, then ask informed questions** — don't ask about things you can discover
- Bring information to the user — don't make them guess
- Reflect back what you heard to confirm understanding
- Push back when scope seems too broad or slices too big
- Let the conversation zoom in and out naturally
- **Every question should be creative and context-specific.** Drive questions from what the user actually said. Never fall back on generic templates. Each question should feel like it came from a thinking partner who's been paying attention.

---

## Start

1. Run `bd list --json` to see existing beads
2. Determine entry point:
   - If the user specified an item → load it (standalone)
   - If the user mentioned recent items → find human-flagged and blocked items (continuation)
   - If neither → query for human-flagged and blocked items and present the list (auto-discover)
3. Announce what artifacts this session will produce
4. Assess where the input is in its lifecycle, diagnose the pass, and begin the conversation
