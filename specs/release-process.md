# Release Process

GitHub Actions workflow creates draft releases with pre-built binaries when version tags are pushed.

## Slice 1: Core Release Workflow with Native Builds

### User Behavior

Developer workflow:
1. Update version in `Cargo.toml` (e.g., `0.2.0`)
2. Commit: `git commit -am "Bump version to 0.2.0"`
3. Tag: `git tag v0.2.0`
4. Push: `git push && git push --tags`
5. GitHub Action triggers, builds binaries, creates draft release
6. Developer reviews draft on GitHub, writes release notes, publishes

### Acceptance Criteria

- [x] LICENSE file exists with MIT license text
- [x] Cargo.toml has metadata: `license`, `repository`, `description`, `homepage`
- [x] Workflow triggers on tags matching `v[0-9]+.[0-9]+.[0-9]+`
- [x] Workflow fails if tag version doesn't match Cargo.toml version
- [x] Draft GitHub release is created with tag name as title
- [x] Binaries built for 4 native targets:
  - [x] macOS Intel (`x86_64-apple-darwin`)
  - [x] macOS Apple Silicon (`aarch64-apple-darwin`)
  - [x] Linux x86_64 (`x86_64-unknown-linux-musl`)
  - [x] Windows x86_64 (`x86_64-pc-windows-msvc`)
- [x] Archives uploaded: `.tar.gz` for Unix, `.zip` for Windows
- [x] SHA256 checksum file uploaded for each archive
- [x] Archive naming: `ralph-{version}-{target}.tar.gz` (or `.zip`)

### Technical Constraints

Follow ripgrep's workflow structure:
- `create-release` job: validates version, creates draft release via `gh release create --draft`
- `build-release` job: matrix build, uploads via `gh release upload`
- `build-release` depends on `create-release`

Build configuration:
- Use `cargo build --release` (consider `--profile release-lto` for smaller binaries)
- Strip binaries for size
- Use musl for Linux (static linking, maximum portability)

Runner mapping:
| Target | Runner |
|--------|--------|
| `x86_64-apple-darwin` | `macos-13` |
| `aarch64-apple-darwin` | `macos-14` |
| `x86_64-unknown-linux-musl` | `ubuntu-latest` |
| `x86_64-pc-windows-msvc` | `windows-latest` |

### Error Cases

- **Tag doesn't match Cargo.toml version**: Workflow fails with clear error message before creating release
- **Invalid tag format** (e.g., `0.2.0` without `v`): Workflow doesn't trigger
- **Build fails on one platform**: Draft release exists, that platform's binary is missing (other platforms still upload)
- **Upload fails**: GitHub Actions will retry; if persistent, draft release is incomplete

## Slice 2: Cross-Compiled ARM Targets

Depends on Slice 1.

### User Behavior

Same workflow as Slice 1. Releases now include ARM binaries for Linux and Windows.

### Acceptance Criteria

- [ ] Linux arm64 binary built (`aarch64-unknown-linux-musl`)
- [ ] Windows arm64 binary built (`aarch64-pc-windows-msvc`)
- [ ] Both archives and checksums uploaded to release

### Technical Constraints

Cross-compilation setup:
- Linux arm64: Use `cross` or install `aarch64-linux-musl` toolchain
- Windows arm64: Install target via `rustup target add aarch64-pc-windows-msvc`

Reference ripgrep's approach for cross-compilation toolchain setup.

### Error Cases

- **Cross-compilation toolchain unavailable**: Build fails, that platform's binary missing
- **QEMU/emulation issues** (if using `cross`): Build fails with toolchain error

## Out of Scope

- Shell completions
- Man pages
- Debian packages (`.deb`)
- Auto-publish (releases are drafts, manually published)
- Auto-generated release notes (notes written manually)
