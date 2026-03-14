---
name: ralph-spec
description: "Spec Shaping Interview. Use when the user wants to shape, define, or write a spec for a feature, bug fix, refactor, or other work item."
---

# Spec Shaping Interview

Shape a specification through conversation. Your goal: produce a spec that an implementing agent can execute in small, valuable vertical slices.

## Setup

**Detect mode** by reading the `.ralph` config file in the project root. Look for `mode = "beads"` or `mode = "specs"` under `[behavior]`.

- **Specs mode** (default): Read `specs/README.md` and `specs/TEMPLATE.md` before starting.
- **Beads mode**: No spec files needed. Output will be `bd create` commands.

**Announce the mode:** Tell the user which mode you detected. Example: *"I see this project uses beads mode, so I'll create beads (issues) instead of spec files. Let me know if you'd prefer specs instead."*

If the user wants to override the detected mode, respect their choice.

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

### Work Items vs Slices

One conversation might produce **multiple work items** (specs in specs mode, beads in beads mode).

- **Work item** = A cohesive feature. "User can do X." Has its own value statement.
- **Slice** = The smallest unit within a work item that still delivers user value.

**Red flag:** If your "slices" deliver unrelated user value, you probably have multiple work items. Each work item should have a clear "User can do X" statement. Slices within that work item break it into smaller deliverables.

### What Makes a Good Slice

A vertical slice:
- Cuts through all layers (not "build API, then UI"—build one thin feature end-to-end)
- Delivers something the user can see, verify, or benefit from
- Works independently, even if limited
- **Is smaller than you think.** One discrete change, not multiple changes bundled together.

### Slice Sizing: Smaller Is Better

**If a slice has "and" in it, it's probably too big.** Break it down.

Bad: "Remove SwiftData and add GRDB"
Good: Two slices — "Remove SwiftData" then "Add GRDB"

Bad: "Set up database connection and migrations"
Good: Two slices — "Set up database connection" then "Add migrations infrastructure"

**Each slice = one focused change.** When in doubt, slice thinner.

### Shape Up Criteria

Use these to prioritize slices:

**Core:** Is this central to the concept?
- "Without this, the other work wouldn't mean anything"
- Do core slices first

**Small:** Can this be completed in one short agent session?
- If not, slice thinner
- Hours of work, not days

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
- **Specs mode:** Review `specs/README.md` for dependencies on other specs
- **Beads mode:** Run `bd list --json` to check for dependencies on existing beads
- Identify which slices depend on other slices
- Check if required code/features already exist

**Propose an order:** "Based on dependencies, I suggest: Slice 1 → Slice 2 → Slice 3. Does this make sense?"

---

## Phase 8: Boundaries

Only if scope is ambiguous:
- "What might someone assume is included that isn't?"
- "What related work are we explicitly deferring?"

---

## Phase 9: Write the Work Items

When the conversation converges:

1. **Summarize** what you're about to create and confirm with the user

### Specs Mode

2. **Create** `specs/[name].md` for each spec, following `specs/TEMPLATE.md`
3. **Update** `specs/README.md`:
   - Add each spec to the table
   - Set status to **Ready**
   - Write one-line summary
   - List dependencies (specs can depend on other specs)

### Beads Mode

2. **Create beads** using `bd create` for each work item. For each bead:
   - Title: Clear, action-oriented (e.g., "Add user authentication")
   - Description: Include the value statement, slices as a checklist, technical constraints, and error handling notes
   - Type: `feature`, `bug`, `task`, `refactor`, etc.
   - Priority: 0-4 (0 = highest)
   - Dependencies: Use `--deps` to link related beads

   Example:
   ```bash
   bd create "Add user authentication" \
     --description="User can log in with email/password.

   ## Slices
   - [ ] Slice 1: Basic login form with email/password fields
   - [ ] Slice 2: Session persistence across page reloads
   - [ ] Slice 3: Error messaging for invalid credentials

   ## Technical Constraints
   - Follow existing auth patterns in the codebase
   - Use bcrypt for password hashing

   ## Error Cases
   - Invalid credentials: show generic 'invalid email or password' message
   - Rate limiting: lock account after 5 failed attempts" \
     -t feature -p 2
   ```

3. **Show the user** the `bd create` commands before running them for confirmation

## Phase 10: Finalize

### Specs Mode
Make a commit once getting approval from the user.

### Beads Mode
Run the confirmed `bd create` commands to create the beads. No file commit needed — beads are tracked by `bd`.

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

1. Read `.ralph` to detect the mode
2. **Specs mode:** Read `specs/README.md` and `specs/TEMPLATE.md`
3. **Beads mode:** Run `bd list --json` to see existing beads for context
4. Announce the detected mode to the user
5. Ask: **"What are we working on?"**
