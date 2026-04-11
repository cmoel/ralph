#!/bin/bash
# PreToolUse hook: routes `bd` commands through scripts/bd-retry.sh so they
# retry on transient embedded-Dolt lock errors.
#
# Handles the two common shapes:
#   bd <args>
#   cd <path> && bd <args>
# Other shapes pass through unchanged.

input=$(cat)
command=$(echo "$input" | jq -r '.tool_input.command')
cwd=$(echo "$input" | jq -r '.cwd')

SCRIPT="scripts/bd-retry.sh"

if [ ! -f "$cwd/$SCRIPT" ]; then
  exit 0
fi

rewrite_bd() {
  case "$1" in
    "bd "*)
      printf 'bash %s %s' "$SCRIPT" "${1#bd }"
      return 0 ;;
    "bd")
      printf 'bash %s' "$SCRIPT"
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
    exit 0
  fi
elif new_rest=$(rewrite_bd "$command"); then
  new_command="$new_rest"
else
  exit 0
fi

echo "$input" | jq --arg cmd "$new_command" '{
  hookSpecificOutput: {
    hookEventName: "PreToolUse",
    permissionDecision: "allow",
    updatedInput: (.tool_input | .command = $cmd)
  }
}'
