---
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

Use AskUserQuestion to push the user to extract ALL ideas. Never let them off easy.

**Exception:** Don't use AskUserQuestion for "anything else?" / "what else?" breadth sweeps. Ask those as normal conversation text so the user can naturally stop responding when they're drained.

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

Before filing, check against existing items by comparing against the `bd list --json` results from setup.

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

**Get confirmation before filing.** Use AskUserQuestion with only the positive action (e.g., "File these 5 beads"). The built-in Other text box already lets the user redirect — don't add filler options like "Adjustments needed."

### Filing

Create beads using `bd create` for each item:

```bash
bd create "Title of work item" \
  --description="One-line description of what this solves.

## Context
Why this matters and what prompted it.

## Open Questions
- Things that need investigation during shaping" \
  -t task -p 2
```

After creating each bead, flag it for human attention so it lands in the shaping queue:

```bash
bd update <id> --add-label=human
```

- Default type: `task` (adjust if clearly a `bug`, `feature`, `refactor`, etc.)
- Default priority: `2` (adjust based on user's emphasis during extraction)
- Always flag for human — these items need /shape before implementation
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
