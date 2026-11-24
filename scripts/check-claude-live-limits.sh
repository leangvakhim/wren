#!/bin/bash
# Wren diagnostic 2: sends ONE tiny message ("Reply with just: OK", Haiku model,
# so it uses very little of your plan) to your Default Claude account with the
# same flags Wren uses, and prints what Claude Code reports about plan limits
# during the reply. No tokens are printed.
# Run it in Terminal:  bash scripts/check-claude-live-limits.sh

if ! command -v claude >/dev/null 2>&1; then
  echo "The claude CLI isn't on PATH in this Terminal." >&2
  exit 1
fi
echo "claude $(claude --version 2>/dev/null)"
out=$(mktemp "${TMPDIR:-/tmp}/wren-live.XXXXXX")
MSG='{"type":"user","message":{"role":"user","content":[{"type":"text","text":"Reply with just: OK"}]}}'
cd "${TMPDIR:-/tmp}" || exit 1
echo "Sending one tiny message (up to ~60 s)…"
( echo "$MSG"
  # keep stdin open until the reply has finished, like Wren does
  for _ in $(seq 1 60); do sleep 1; grep -q '"type":"result"' "$out" 2>/dev/null && break; done
) | env CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_TELEMETRY=1 DISABLE_ERROR_REPORTING=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1 \
    claude -p --input-format stream-json --output-format stream-json --verbose --include-partial-messages \
      --permission-mode default --permission-prompts none --model haiku --no-session-persistence > "$out" 2>/dev/null

echo
echo "--- message types Claude Code sent (count, type)"
grep -o '"type":"[a-z_]*"' "$out" | sort | uniq -c | sort -rn | head -12
echo
echo "--- rate_limit_event lines (these carry the 5-hour / weekly numbers)"
if grep -q '"rate_limit_event"' "$out"; then
  grep '"rate_limit_event"' "$out" | cut -c1-1200
else
  echo "(none)"
fi
echo
echo "--- end of the reply"
grep '"type":"result"' "$out" | sed -E 's/"modelUsage":\{.*\},"permission_denials"/"modelUsage":{…},"permission_denials"/' | cut -c1-600
rm -f "$out"
