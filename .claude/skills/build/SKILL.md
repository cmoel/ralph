---
name: build
description: "Build ralph, sign the binary, and install it. Use when the user wants to build, compile, install, or deploy ralph. Accepts 'development' or 'release' as an argument (defaults to release)."
---

# Build

Build ralph, ad-hoc codesign the binary, and install it to ~/.bin.

## Arguments

- `release` (default): Optimized release build via `cargo build --release`
- `development`: Debug build via `devbox run build`

## Steps

1. **Check** that the working tree is clean — no uncommitted changes allowed.
2. **Build** the binary using the appropriate profile.
3. **Sign** the binary with `codesign -f -s -` *after* installing (macOS invalidates signatures on copy).
4. **Install** to `~/.bin/ralph`.
5. **Verify** with `~/.bin/ralph --version`.

## Important

- Always sign *after* copying to `~/.bin` — macOS strips the signature on file copy.
- Use `codesign -f -s -` (force flag) to replace any existing signature.
- Always remove quarantine after signing — `xattr -dr com.apple.quarantine ~/.bin/ralph` — to prevent Gatekeeper blocks.

## Execution

Determine the build mode from the argument. If no argument is provided, default to `release`.

IMPORTANT: Run each step as its own separate Bash call — do NOT chain commands with &&.

### Dirty check (both modes)

Run this before anything else. If the output is non-empty, **stop immediately** and tell the user:

> Build aborted: working tree is dirty. Commit or stash your changes before building.

```bash
git status --porcelain
```

### Release

```bash
cargo build --release
```
```bash
cp target/release/ralph ~/.bin/ralph
```
```bash
codesign -f -s - ~/.bin/ralph
```
```bash
xattr -dr com.apple.quarantine ~/.bin/ralph
```
```bash
~/.bin/ralph --version
```

### Development

```bash
devbox run build
```
```bash
cp target/debug/ralph ~/.bin/ralph
```
```bash
codesign -f -s - ~/.bin/ralph
```
```bash
xattr -dr com.apple.quarantine ~/.bin/ralph
```
```bash
~/.bin/ralph --version
```
