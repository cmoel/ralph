# Project README

README.md in repo root documenting what ralph is, how to install, configure, and contribute.

## User Behavior

Developers visiting the GitHub repo can:
- Understand what ralph does at a glance (description + GIF)
- Install ralph (from releases or source)
- Configure ralph for their setup
- Contribute to development

## Acceptance Criteria

- [ ] README.md exists in repo root
- [ ] One-line description explains what ralph is
- [ ] GIF shows ralph in action (streaming output, tool summaries)
- [ ] Installation section covers:
  - [ ] Download from GitHub releases (link to releases page)
  - [ ] Build from source (`cargo build --release`)
- [ ] Usage section explains:
  - [ ] How to run ralph
  - [ ] What ralph expects (PROMPT.md, specs directory)
- [ ] Configuration section documents:
  - [ ] Config file location (platform-specific)
  - [ ] Key options: `claude.path`, `paths.prompt`, `paths.specs`, `behavior.auto_continue`
  - [ ] Environment variable overrides
  - [ ] Points to config file for full options
- [ ] Contributing section includes:
  - [ ] devbox commands (`build`, `test`, `check`, `fmt`)
  - [ ] Link to specs directory for feature development
- [ ] License section states MIT

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
