---
name: shape
description: "Deep shaping session for work items. Use when the user wants to shape, refine, specify, or detail a work item, bead, or spec using Shape Up methodology."
---

# Shape

Deeply refine rough work items into fully shaped, implementation-ready specifications using Shape Up methodology. Your goal: take a vague or under-specified item and produce something an implementing agent can execute in small, valuable vertical slices.

## Setup

**Detect mode** by reading the `.ralph` config file in the project root. Look for `mode = "beads"` or `mode = "specs"` under `[behavior]`.

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

1. Read `.ralph` to detect the mode
2. **Beads mode:** Run `bd list --json` to see existing beads
3. **Specs mode:** Read `specs/README.md` and `specs/TEMPLATE.md`
4. Determine entry point:
   - If the user specified an item → load it (standalone)
   - If the user mentioned recent items → find `needs-shaping` items (continuation)
   - If neither → query for `needs-shaping` items and present the list (auto-discover)
5. Announce the detected mode
6. Summarize what you see in the item, then begin the shaping conversation
