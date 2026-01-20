# Project README

README.md in repo root documenting what ralph is, how to install, configure, and contribute.

## User Behavior

Developers visiting the GitHub repo can:
- Understand what ralph does at a glance (description + GIF)
- Install ralph (from releases or source)
- Configure ralph for their setup
- Contribute to development

## Acceptance Criteria

- [x] README.md exists in repo root
- [x] One-line description explains what ralph is
- [ ] GIF shows ralph in action (streaming output, tool summaries)
- [x] Installation section covers:
  - [x] Download from GitHub releases (link to releases page)
  - [x] Build from source (`cargo build --release`)
- [x] Usage section explains:
  - [x] How to run ralph
  - [x] What ralph expects (PROMPT.md, specs directory)
- [x] Configuration section documents:
  - [x] Config file location (platform-specific)
  - [x] Key options: `claude.path`, `paths.prompt`, `paths.specs`, `behavior.iterations`
  - [x] Environment variable overrides
  - [x] Points to config file for full options
- [x] Contributing section includes:
  - [x] devbox commands (`build`, `test`, `check`, `fmt`)
  - [x] Link to specs directory for feature development
- [x] License section states MIT

Note: GIF placeholder created at `assets/demo.gif`. Actual recording requires manual creation with vhs or asciinema.

## Technical Constraints

**GIF creation:**
- Use `vhs` (charmbracelet/vhs) or `asciinema` to record terminal session
- Store GIF in repo (e.g., `assets/demo.gif`)
- Keep recording short (10-20 seconds) showing:
  - Ralph starting
  - Streaming output appearing
  - Tool use summaries
- Update GIF when UI changes significantly

**README structure:**
```markdown
# ralph

One-line description.

![Demo](assets/demo.gif)

## Installation
## Usage
## Configuration
## Contributing
## License
```

**Config file locations by platform:**
- Linux: `~/.config/ralph/config.toml`
- macOS: `~/Library/Application Support/dev.cmoel.ralph/config.toml`
- Windows: `C:\Users\<User>\AppData\Roaming\cmoel\ralph\config\config.toml`

## Error Cases

Not applicable — this is documentation.

## Out of Scope

- Detailed API documentation
- Changelog (can be added later)
- Badges (CI status, version, etc. — can be added after release-process is done)

## Dependencies

- [release-process](release-process.md) — Installation section references GitHub releases
