#!/bin/bash
# Retry `bd` commands on transient embedded-Dolt lock errors.
# Usage: bd-retry.sh <bd args...>
#
# Retries with exponential backoff when bd fails because another process
# holds the exclusive Dolt lock (e.g. the ralph TUI mid-refresh). Non-lock
# failures are passed through immediately.

set -u

delays=(0.2 0.4 0.8 1.6 3.2)
attempt=0
tmp_out=$(mktemp)
tmp_err=$(mktemp)
trap 'rm -f "$tmp_out" "$tmp_err"' EXIT

while :; do
  : > "$tmp_out"
  : > "$tmp_err"
  bd "$@" > "$tmp_out" 2> "$tmp_err"
  rc=$?

  if [ $rc -eq 0 ]; then
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit 0
  fi

  if ! grep -qE "failed to open database|another process holds the exclusive lock" "$tmp_err"; then
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit $rc
  fi

  if [ $attempt -ge ${#delays[@]} ]; then
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit $rc
  fi

  sleep "${delays[$attempt]}"
  attempt=$((attempt + 1))
done
