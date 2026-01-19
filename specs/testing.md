# Testing

Add unit tests for pure logic. Refactor code to separate pure functions from I/O where needed.

## Principle

**Functional core, imperative shell.** Extract pure logic into testable functions. Keep file/process/terminal I/O in thin wrappers that call the pure functions.

## Slice 1: Test Pure Functions (No Refactoring)

Test functions that are already pure and testable.

### Acceptance Criteria

- [ ] `ui.rs`: Test `format_elapsed()` — various durations, edge cases
- [ ] `ui.rs`: Test `truncate_str()` — short strings, long strings, newlines
- [ ] `ui.rs`: Test `format_with_thousands()` — 0, hundreds, thousands, millions
- [ ] `specs.rs`: Test `SpecStatus::from_str()` — all variants, invalid input
- [ ] `specs.rs`: Test `SpecStatus::label()` — all variants
- [ ] `modals.rs`: Test `ConfigModalField::next()` and `prev()` — full cycle, wraparound
- [ ] All tests pass with `devbox run test`

### Technical Constraints

- Add `#[cfg(test)]` module at bottom of each file
- Follow existing test style in `config.rs`

## Slice 2: Extract and Test Spec Parsing

Refactor spec parsing to separate pure logic from file I/O, then test the pure functions.

### Current Problem

Three functions read specs/README.md with different approaches:
- `parse_specs_readme()` — proper table parsing
- `check_specs_remaining()` — fragile `.contains("| Ready |")`
- `detect_current_spec()` — fragile `.contains("| In Progress |")`

### Refactoring

Extract pure parsing function:

```rust
/// Parse specs table from README content (pure function).
fn parse_specs_table(contents: &str) -> Vec<ParsedSpec> { ... }

/// Thin I/O wrapper.
pub fn parse_specs_readme(specs_dir: &Path) -> Result<Vec<SpecEntry>, String> {
    let contents = std::fs::read_to_string(specs_dir.join("README.md"))?;
    // ... call parse_specs_table, add timestamps
}
```

Consolidate the other two functions to use the same parser:

```rust
pub fn check_specs_remaining(specs_dir: &Path) -> SpecsRemaining {
    match parse_specs_readme(specs_dir) {
        Ok(specs) => {
            if specs.iter().any(|s| matches!(s.status, SpecStatus::Ready | SpecStatus::InProgress)) {
                SpecsRemaining::Yes
            } else {
                SpecsRemaining::No
            }
        }
        Err(_) => SpecsRemaining::Missing,
    }
}
```

### Acceptance Criteria

- [ ] `parse_specs_table(contents: &str)` extracted as pure function
- [ ] `check_specs_remaining()` uses `parse_specs_readme()` instead of string matching
- [ ] `detect_current_spec()` uses `parse_specs_readme()` instead of string matching
- [ ] Tests for `parse_specs_table()`:
  - [ ] Parses valid table row
  - [ ] Handles all status variants (Ready, In Progress, Done, Blocked)
  - [ ] Skips header rows and separator lines
  - [ ] Handles missing/malformed rows gracefully
  - [ ] Handles whitespace variations (` | Ready |` vs `|Ready|`)
- [ ] All existing functionality preserved
- [ ] `devbox run test` and `devbox run check` pass

## Slice 3: Extract and Test Validators

Refactor validators to separate logic from filesystem checks.

### Current Problem

Validators directly call `std::fs::metadata()`:

```rust
pub fn validate_executable_path(path: &str) -> Option<String> {
    let expanded = Config::expand_tilde(path);
    match std::fs::metadata(&expanded) { ... }
}
```

### Refactoring

Extract validation logic:

```rust
/// Check if metadata indicates an executable file (pure function).
fn check_executable_metadata(metadata: &std::fs::Metadata) -> Option<String> { ... }

/// Thin I/O wrapper.
pub fn validate_executable_path(path: &str) -> Option<String> {
    let expanded = Config::expand_tilde(path);
    match std::fs::metadata(&expanded) {
        Ok(metadata) => check_executable_metadata(&metadata),
        Err(e) => Some(error_message_for(e)),
    }
}
```

### Acceptance Criteria

- [ ] Validation logic extracted from I/O for all three validators
- [ ] Tests for validation logic (using constructed metadata or trait abstraction)
- [ ] `devbox run test` and `devbox run check` pass

## Out of Scope

- Integration tests (temp files, mock processes)
- UI rendering tests (would require terminal mocking)
- Event processing tests (tightly coupled to App state — defer until needed)
- 100% coverage (test what matters, not everything)

## Dependencies

- [code-organization](code-organization.md) (Done)
