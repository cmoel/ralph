#!/bin/bash
# Retry `bd` commands on transient embedded-Dolt lock errors.
# Usage: bd-retry.sh <bd args...>
#
# Retries with exponential backoff when bd fails because another process
# holds the exclusive Dolt lock (e.g. the ralph TUI mid-refresh). Non-lock
# failures are passed through immediately.

set -u

LOG="/tmp/ralph-bd-retry.log"
ts() { date +"%Y-%m-%dT%H:%M:%S.%3N"; }

delays=(0.2 0.4 0.8 1.2 2.0 3.0 3.0 3.0 3.0)
attempt=0
tmp_out=$(mktemp)
tmp_err=$(mktemp)
trap 'rm -f "$tmp_out" "$tmp_err"' EXIT

printf '[%s] START pid=%s args=%q\n' "$(ts)" "$$" "$*" >> "$LOG" 2>/dev/null || true

while :; do
  : > "$tmp_out"
  : > "$tmp_err"
  bd "$@" > "$tmp_out" 2> "$tmp_err"
  rc=$?

  if [ $rc -eq 0 ]; then
    printf '[%s] OK pid=%s attempt=%d\n' "$(ts)" "$$" "$attempt" >> "$LOG" 2>/dev/null || true
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit 0
  fi

  # Check both streams — bd has been observed to emit the lock error to stdout.
  if ! grep -qE "failed to open database|another process holds the exclusive lock" "$tmp_err" "$tmp_out"; then
    printf '[%s] FAIL_NONTRANSIENT pid=%s attempt=%d rc=%d\n' "$(ts)" "$$" "$attempt" "$rc" >> "$LOG" 2>/dev/null || true
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit $rc
  fi

  if [ $attempt -ge ${#delays[@]} ]; then
    printf '[%s] FAIL_EXHAUSTED pid=%s attempts=%d rc=%d\n' "$(ts)" "$$" "$attempt" "$rc" >> "$LOG" 2>/dev/null || true
    cat "$tmp_out"
    cat "$tmp_err" >&2
    exit $rc
  fi

  printf '[%s] RETRY pid=%s attempt=%d delay=%s\n' "$(ts)" "$$" "$attempt" "${delays[$attempt]}" >> "$LOG" 2>/dev/null || true
  sleep "${delays[$attempt]}"
  attempt=$((attempt + 1))
done
