#!/bin/bash
# PreToolUse hook: routes `bd` commands through scripts/bd-retry.sh so they
# retry on transient embedded-Dolt lock errors.
#
# Handles the two common shapes:
#   bd <args>
#   cd <path> && bd <args>
# Other shapes pass through unchanged.

LOG="/tmp/ralph-intercept-bd.log"
ts() { date +"%Y-%m-%dT%H:%M:%S.%3N"; }

input=$(cat)
command=$(echo "$input" | jq -r '.tool_input.command')
cwd=$(echo "$input" | jq -r '.cwd')

printf '[%s] INVOKED cwd=%s command=%q\n' "$(ts)" "$cwd" "$command" >> "$LOG" 2>/dev/null || true

# Prefer $CLAUDE_PROJECT_DIR (set by Claude Code for hooks) so the rewrite
# works even when the agent has cd'd into a sub-directory or worktree.
PROJECT_DIR="${CLAUDE_PROJECT_DIR:-$cwd}"
SCRIPT="$PROJECT_DIR/scripts/bd-retry.sh"

if [ ! -f "$SCRIPT" ]; then
  printf '[%s] SKIP_NO_SCRIPT path=%s\n' "$(ts)" "$SCRIPT" >> "$LOG" 2>/dev/null || true
  exit 0
fi

rewrite_bd() {
  case "$1" in
    "bd "*)
      printf 'bash "%s" %s' "$SCRIPT" "${1#bd }"
      return 0 ;;
    "bd")
      printf 'bash "%s"' "$SCRIPT"
      return 0 ;;
  esac
  return 1
}

if [[ "$command" == "cd "*" && bd "* ]] || [[ "$command" == "cd "*" && bd" ]]; then
  prefix="${command%% && bd*}"
  rest="bd${command#*&& bd}"
  if new_rest=$(rewrite_bd "$rest"); then
    new_command="$prefix && $new_rest"
  else
    printf '[%s] SKIP_NO_MATCH_CD command=%q\n' "$(ts)" "$command" >> "$LOG" 2>/dev/null || true
    exit 0
  fi
elif new_rest=$(rewrite_bd "$command"); then
  new_command="$new_rest"
else
  printf '[%s] SKIP_NO_MATCH command=%q\n' "$(ts)" "$command" >> "$LOG" 2>/dev/null || true
  exit 0
fi

printf '[%s] REWRITE from=%q to=%q\n' "$(ts)" "$command" "$new_command" >> "$LOG" 2>/dev/null || true

echo "$input" | jq --arg cmd "$new_command" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "allow",
    updatedInput: (.tool_input | .command = $cmd)
  }
}'
