# ralph

TUI wrapper for `claude` CLI that displays formatted streaming output.

![Demo](assets/demo.gif)

## Installation

### Pre-built Binaries

Download the latest release for your platform from the [GitHub Releases page](https://github.com/cmoel/ralph/releases).

Available platforms:
- macOS (Intel and Apple Silicon)
- Linux (x86_64 and ARM64, musl)
- Windows (x86_64 and ARM64)

### Build from Source

```bash
git clone https://github.com/cmoel/ralph.git
cd ralph
cargo build --release
```

The binary will be at `target/release/ralph`.

## Usage

Run ralph from a directory containing:
- `PROMPT.md` — The prompt file to pass to claude
- `specs/` — A directory containing your specification files

```bash
ralph
```

Ralph spawns the `claude` CLI, streams its JSON output, and renders it as formatted text with tool use summaries and token usage.

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `q` | Quit |
| `s` | Start/Stop |
| `l` | Open specs panel |
| `c` | Open config modal |
| `j/↓` | Scroll down |
| `k/↑` | Scroll up |
| `Ctrl+d` | Scroll down half page |
| `Ctrl+u` | Scroll up half page |
| `Ctrl+f` | Scroll down full page |
| `Ctrl+b` | Scroll up full page |

## Configuration

Ralph uses a TOML configuration file. On first run, a default config is created.

### Config File Location

| Platform | Path |
|----------|------|
| Linux | `~/.config/ralph/config.toml` |
| macOS | `~/Library/Application Support/ralph/config.toml` |
| Windows | `%APPDATA%\cmoel\ralph\config.toml` |

### Options

```toml
[claude]
path = "~/.claude/local/claude"  # Path to claude CLI

[paths]
prompt = "./PROMPT.md"  # Prompt file path
specs = "./specs"       # Specs directory path

[logging]
level = "info"  # Log level: debug, info, warn, error

[behavior]
iterations = -1  # -1 = infinite, 0 = stopped, N = run N times
```

### Environment Variables

Environment variables override config file values:

| Variable | Overrides |
|----------|-----------|
| `RALPH_CLAUDE_PATH` | `claude.path` |
| `RALPH_PROMPT_PATH` | `paths.prompt` |
| `RALPH_SPECS_DIR` | `paths.specs` |
| `RALPH_LOG` | `logging.level` |

## Contributing

Ralph uses [devbox](https://www.jetify.com/devbox) for development.

```bash
devbox run build    # Compile
devbox run test     # Run tests
devbox run check    # Run clippy (must pass before commit)
devbox run fmt      # Format code
```

See the [specs/](specs/) directory for feature specifications and project roadmap.

## License

MIT
