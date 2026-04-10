#!/bin/bash
# PreToolUse hook: rewrites build/test/check/fmt commands through run-silent.sh.
# Invoked via `bash` so the wrapper runs regardless of executable bit on disk.

input=$(cat)
command=$(echo "$input" | jq -r '.tool_input.command')
cwd=$(echo "$input" | jq -r '.cwd')

SCRIPT="scripts/run-silent.sh"

if [ ! -f "$cwd/$SCRIPT" ]; then
  exit 0
fi

case "$command" in
  "devbox run build"|"cargo build")
    desc="build" ;;
  "devbox run test"|"cargo test")
    desc="tests" ;;
  "devbox run check"|"cargo clippy -- -D warnings"|"cargo clippy")
    desc="clippy" ;;
  "devbox run fmt"|"cargo fmt")
    desc="fmt" ;;
  *)
    exit 0 ;;
esac

new_command="bash $SCRIPT \"$desc\" $command"

echo "$input" | jq --arg cmd "$new_command" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "allow",
    updatedInput: (.tool_input | .command = $cmd)
  }
}'
