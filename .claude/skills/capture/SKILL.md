---
name: capture
description: "Quick idea parking. Use when the user wants to capture, park, jot down, or note an idea as a bead without a full brain-dump session."
---

# Capture

Park an idea as a bead immediately. File first, enrich after. The user can leave at any point — the idea is already safe.

## Flow

### 1. File Immediately

Extract a title from whatever the user said (inline args, voice transcript, raw text) and file a bead:

```bash
bd q "Title extracted from user input" -t task -p 2
```

Then label it for the shaping queue:

```bash
bd update <id> --add-label=needs-brain-dump
```

Confirm the bead was filed: show the ID and title.

### 2. Enrich (Optional)

Ask one or two follow-up questions as normal conversation text — not AskUserQuestion. The bead is already filed, so these are free upside. The user can stop responding at any time.

Use your judgment on what's most useful to ask. Good enrichment questions add context an implementer would need later:
- "What's the actual problem this solves?"
- "Is this related to anything you're already working on?"
- "How urgent — is this blocking something?"

After each answer, update the bead with what you learned:

```bash
bd update <id> --description="Updated description with new context"
```

Don't ask more than two questions. This isn't a brain dump — if there's more to explore, suggest `/brain-dump` or `/shape`.

## Boundaries

- **File before asking.** Never block on user input before creating the bead.
- **No scripted sequences.** The agent decides what (if anything) to ask based on what the user said.
- **No shaping.** This skill produces rough beads. `/brain-dump` and `/shape` do the heavy lifting.
- **Default type `task`, default priority `2`.** Adjust only if the user's input makes it obvious (e.g., "bug" → `-t bug`).
