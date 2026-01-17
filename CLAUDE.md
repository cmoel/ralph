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

## Specs

Read `specs/README.md` before implementing any feature.

## Testing

See `TESTING.md` for testing philosophy.
