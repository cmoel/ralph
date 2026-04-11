# ralph

TUI wrapper for `claude` CLI that displays formatted streaming output.

## Stack

Rust + Ratatui + tokio + crossterm

## Commands

```bash
devbox run build    # compile
devbox run test     # test
devbox run check    # clippy (must pass before commit)
devbox run fmt      # format
```

## Deleting Files

Never use `rm`. To delete a file:
1. Commit any pending changes first
2. Use `git rm <file>` to remove and stage the deletion
3. Commit the deletion

## Architecture

- Async event loop with tokio
- Messages are typed enums (exhaustive match required)
- Logging via tracing with file rotation

## Build Output

Use `scripts/run-silent.sh` for build/test/check/fmt commands to keep output concise. A PreToolUse hook does this automatically for `devbox run build|test|check|fmt` and their cargo equivalents.

## Testing

See `TESTING.md` for testing philosophy.


<!-- BEGIN BEADS INTEGRATION v:1 profile:minimal hash:ca08a54f -->
## Beads Issue Tracker

This project uses **bd (beads)** for issue tracking. Run `bd prime` to see full workflow context and commands.

### Quick Reference

```bash
bd ready              # Find available work
bd show <id>          # View issue details
bd update <id> --claim  # Claim work
bd close <id>         # Complete work
```

### Rules

- Use `bd` for ALL task tracking — do NOT use TodoWrite, TaskCreate, or markdown TODO lists
- Run `bd prime` for detailed command reference and session close protocol
- Use `bd remember` for persistent knowledge — do NOT use MEMORY.md files

## Session Completion

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:
   ```bash
   git pull --rebase
   bd dolt push
   git push
   git status  # MUST show "up to date with origin"
   ```
5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**
- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds
<!-- END BEADS INTEGRATION -->
