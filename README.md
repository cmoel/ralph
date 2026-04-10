# Ralph

A TUI for running [Ralph loops](https://ghuntley.com/ralph/).

![Ralph TUI](assets/screenshot.png)

## Installation

### Homebrew (macOS)

```bash
brew install cmoel/tap/ralph
```

### Pre-built Binaries

Download from [GitHub Releases](https://github.com/cmoel/ralph/releases):

- macOS (Intel and Apple Silicon)
- Linux (x86_64 and ARM64)
- Windows (x86_64 and ARM64)

### Build from Source

```bash
git clone https://github.com/cmoel/ralph.git
cd ralph
cargo build --release
```

Binary: `target/release/ralph`

## Prerequisites

- [Claude CLI](https://docs.anthropic.com/en/docs/claude-code) installed and authenticated
- [Beads](https://github.com/gastownhall/beads) (`bd` CLI) >= 1.0

## Quick Start

```bash
cd your-project
ralph init       # scaffolds .claude/skills/ with brain-dump, shape, capture
ralph            # launches the TUI (kanban board)
```

Ralph uses a compiled-in default prompt. To override it, place a `PROMPT.md` in your per-project config directory (see [Configuration](#configuration)).

## How It Works

Ralph spawns Claude Code (one or more concurrent workers, configurable), claims ready beads from `bd`, streams structured output, and auto-continues iterations until no claimable work remains. The kanban board is the primary view — press `w` to watch live worker output in a modal, and `?` to see help for whatever view you're currently in. `ralph init` scaffolds the three brain-dump, shape, and capture skills into `.claude/skills/` so you can invoke them as Claude slash commands.

## CLI Subcommands

| Command | Purpose |
|---|---|
| `ralph` | Launch the TUI |
| `ralph init` | Scaffold or refresh `.claude/skills/` with drift detection |
| `ralph doctor` | Health check: config, Claude CLI, PROMPT.md, bd, skill drift, board_columns.toml, Dolt |
| `ralph ready [-v]` | List beads claimable by the loop; `-v` shows skip reasons |
| `ralph logs [--id ID] [--path]` | Dump session logs to stdout or print the log directory |
| `ralph tool history [flags]` | Query the tool call history database |
| `ralph tool allow <pattern> [--project]` | Allow a tool pattern in Claude settings |
| `ralph tool deny <pattern> [--project]` | Deny a tool pattern in Claude settings |
| `ralph tool list` | List all tool permissions across settings files |

## Keyboard Shortcuts

The canonical, always-current reference is the in-app help (`?`), which is scoped to whichever view or modal you're in. The tables below give new users a complete reference before launching Ralph.

### Global

These keys work from the board, and where noted, from inside modals.

| Key | Action |
|-----|--------|
| `S` | Start/stop the loop |
| `q` | Quit (confirmation if stopped; hint if running) |
| `c` | Open config modal |
| `i` | Open init modal |
| `D` | Toggle Dolt server |
| `w` | Open workers stream modal |
| `?` | Open context-aware help for the current view |

### Kanban Board

| Key | Action |
|-----|--------|
| `h` / `←` | Previous column |
| `l` / `→` | Next column |
| `k` / `↑` | Previous card |
| `j` / `↓` | Next card |
| `Enter` | Focus preview pane |
| `X` | Close selected bead (with optional reason) |
| `d` | Defer selected bead (with optional until date) |
| `b` | Add dependency (`1` = blocked-by, `2` = blocks) |
| `+` / `=` | Raise priority |
| `-` | Lower priority |
| `H` | Toggle `human` label |
| `u` | Undo last board action |
| `Ctrl+r` | Redo |

### Preview Pane

After pressing `Enter` from the board:

| Key | Action |
|-----|--------|
| `j` / `↓` | Scroll down |
| `k` / `↑` | Scroll up |
| `Esc` / `Enter` | Back to board |

### Workers Stream Modal

Press `w` to open.

| Key | Action |
|-----|--------|
| `k` / `↑` | Previous worker |
| `j` / `↓` | Next worker |
| `g` | Scroll to top |
| `G` | Scroll to bottom (re-enables auto-follow) |
| `Ctrl+u` | Scroll up 10 lines |
| `Ctrl+d` | Scroll down 10 lines |
| `Esc` | Close modal |

### Config Modal

Press `c` to open.

`Tab` / `Shift+Tab` move between fields. `←` / `→` adjust the current field or move the cursor. `Enter` saves on the Save button and cancels on the Cancel button. `Esc` closes without saving.

### Init Modal

Press `i` to open.

`Tab` / `←` / `→` switch between Initialize and Cancel. `Enter` confirms. `Esc` cancels.

### Quit Confirmation

`y` / `Y` to quit, `n` / `N` / `Esc` to cancel.

## Configuration

Ralph reads per-project config from the platform config directory, not from the repo. The path is derived deterministically from the current working directory:

- **macOS:** `~/Library/Application Support/com.cmoel.ralph/projects/<key>/config.toml`
- **Linux:** `~/.config/ralph/projects/<key>/config.toml`
- **Windows:** `%APPDATA%\cmoel\ralph\projects\<key>\config.toml`

`<key>` is the absolute cwd with path separators replaced by `-` (e.g., `/Users/alice/code/ralph` → `-Users-alice-code-ralph`). Edit via `c` in the TUI or the file directly. Full example:

```toml
[claude]
path = "~/.claude/local/claude"

[logging]
level = "info"

[behavior]
iterations = -1        # -1 = infinite, 0 = stopped, N>0 = run N then stop
keep_awake = true
bd_path = "bd"
workers = 1            # concurrent Claude Code workers
heartbeat_interval = 30
stale_threshold = 180
```

Per-project `PROMPT.md` and `board_columns.toml` live alongside `config.toml` in the same directory. Both fall back to compiled-in defaults when absent.

## Environment Variables

| Variable | Overrides |
|----------|-----------|
| `RALPH_CLAUDE_PATH` | `claude.path` |
| `RALPH_BD_PATH` | `behavior.bd_path` |
| `RALPH_LOG` | `logging.level` (also locks the config modal from changing it) |

## Logs

Daily-rotating files, 7-day retention:

- **macOS:** `~/Library/Logs/ralph/`
- **Linux:** `~/.local/state/ralph/`
- **Windows:** `%LocalAppData%\ralph\`

## Contributing

Ralph uses [devbox](https://www.jetify.com/devbox) for development.

```bash
devbox run build    # Compile
devbox run test     # Run tests
devbox run check    # Run clippy
devbox run fmt      # Format code
```

## License

MIT
