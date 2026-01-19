# Dead Code Cleanup

Remove dead code and unnecessary `#[allow(dead_code)]` annotations.

## Remove Unnecessary Annotations

These have `#[allow(dead_code)]` but are actually used:

| Item | Location | Used by |
|------|----------|---------|
| `ConfigLoadStatus::Error` | config.rs:17 | config.rs error handling |
| `specs_path()` | config.rs:124 | app.rs |
| `log_directory` | app.rs:96 | modal_ui.rs |

### Acceptance Criteria

- [x] Remove `#[allow(dead_code)]` from `ConfigLoadStatus::Error` (kept with clearer comment - inner String is only used via Debug)
- [x] Remove `#[allow(dead_code)]` and outdated comment from `specs_path()`
- [x] Remove `#[allow(dead_code)]` from `log_directory`

## Delete Dead Code

These are truly dead — defined but never used:

| Item | Location |
|------|----------|
| `contract_path()` | ui.rs:46-54 |
| `AppStatus::label()` | app.rs:31-38 |
| `logging_error` field | app.rs:99-100 |
| `config_load_status` field | app.rs:106-107 |

### Acceptance Criteria

- [x] Delete `contract_path()` function from ui.rs
- [x] Delete `AppStatus::label()` method from app.rs
- [x] Delete `logging_error` field from App struct and all usages
- [x] Delete `config_load_status` field from App struct and all usages
- [x] `devbox run test` passes
- [x] `devbox run check` passes (no new warnings)

## Keep As-Is

These `#[allow(dead_code)]` annotations are intentional:

- **events.rs types** — Serde deserialization requires fields to exist even if not accessed
- **config.rs `args` field** — Legacy backwards compatibility for old config files

## Dependencies

None
