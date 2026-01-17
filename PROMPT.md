# Agent Workflow

Complete ONE vertical slice per session. A vertical slice delivers observable value to the end user—never build infrastructure without connecting it to user-facing behavior.

## Context Engineering

Your context window is precious. Protect it aggressively.

**Use subagents for:**
- Reading multiple files (spawn parallel readers, get summaries back)
- Searching the codebase (don't let grep results pollute your context)
- Exploring unfamiliar code (let the subagent summarize what matters)
- Any task where you need information but not the raw details

**Keep in your context:**
- The current spec you're implementing
- The code you're actively writing
- Errors you're actively debugging

When in doubt, spawn a subagent. A clean context beats a complete one.

## 1. Discover

Read `specs/README.md` to understand project state.

Spawn parallel subagents to:
- Read all incomplete specs (each subagent reads one, returns summary)
- Optionally read completed specs for context

Select ONE spec (or one slice within a spec) to work on. The slice you find most interesting is fine.

## 2. Understand

Spawn a subagent to read and summarize the selected spec. Have it identify:
- What user-facing behavior this delivers
- Key implementation requirements
- Dependencies on other specs or existing code

Use `IMPLEMENTATION_PLAN.md` as scratch space—delete its contents freely.

## 3. Search

Before writing code, spawn parallel subagents to search the codebase:
- Existing implementations you can extend
- Patterns to follow
- Code your changes might affect

Never assume something isn't implemented.

## 4. Implement

Build the vertical slice. As you work:
- Mark completed items in the spec with `[x]`
- Keep `specs/README.md` accurate
- Keep `IMPLEMENTATION_PLAN.md` current

**For complex implementations:** Break into sub-tasks and use subagents for research-heavy steps. Keep your context focused on the code you're writing.

**If blocked:** Stop and document in BOTH the spec AND `specs/README.md`:
- What failed (exact error or situation)
- Why it's blocking
- 2-3 reasonable options to resolve

## 5. Validate

Before committing:
```bash
devbox run test     # all tests must pass
devbox run fmt      # format all files
devbox run check    # clippy must pass
```

Do not commit until all three pass.

## 6. Commit

When the slice is complete and validated:
1. Mark the spec (or slice) complete in `specs/README.md`
2. Commit with a clear message describing the user-facing change

## 7. Exit

After committing ONE vertical slice, exit immediately. Do not start another task.
