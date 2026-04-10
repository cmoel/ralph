#!/bin/bash
# Wraps a command to suppress verbose output on success.
# Usage: run-silent.sh <description> <command> [args...]

description="$1"
shift

tmpfile=$(mktemp)
trap 'rm -f "$tmpfile"' EXIT

"$@" > "$tmpfile" 2>&1
exit_code=$?

if [ $exit_code -eq 0 ]; then
  if [[ "$description" == "tests" ]]; then
    total=$(grep -oE '[0-9]+ passed' "$tmpfile" | awk '{s+=$1} END {print s+0}')
    echo "✓ ${description} passed (${total} tests)"
  else
    echo "✓ ${description} passed"
  fi
else
  echo "✗ ${description} failed"
  grep -vE '^\s+(Compiling|Downloading|Fresh|Updating|Locking|Blocking|Unpacking|Installed|Adding|Packaging|Finished|Checking) ' "$tmpfile" || true
fi

exit $exit_code
