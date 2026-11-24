#!/bin/bash
# Wren diagnostic: asks Claude Code for its plan limits exactly the way Wren
# does (same flags and privacy settings) and prints the raw answer.
# Read-only: no prompt is sent (costs nothing) and no tokens are printed.
# Run it in Terminal:  bash scripts/check-claude-limits.sh

REQ='{"type":"control_request","request_id":"wren-usage","request":{"subtype":"get_usage","skip_behaviors":true}}'
WREN_DIR="$HOME/Library/Application Support/com.wren.desktop/accounts/claude"

if ! command -v claude >/dev/null 2>&1; then
  echo "The claude CLI isn't on PATH in this Terminal." >&2
  exit 1
fi
echo "claude $(claude --version 2>/dev/null)"

check() { # $1 = label, $2 = account folder (empty = the Default account)
  echo
  echo "=== $1"
  local envs=(CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 DISABLE_TELEMETRY=1 DISABLE_ERROR_REPORTING=1 CLAUDE_CODE_DISABLE_FEEDBACK_SURVEY=1)
  [ -n "$2" ] && envs+=("CLAUDE_CONFIG_DIR=$2")
  echo "--- sign-in"
  env "${envs[@]}" claude auth status --json 2>&1 | grep -E '"(loggedIn|authMethod|subscriptionType|email|apiProvider)"'
  local log
  log=$(mktemp "${TMPDIR:-/tmp}/wren-usage.XXXXXX")
  echo "--- answer to get_usage (takes ~15 s)"
  (echo "$REQ"; sleep 15) \
    | env "${envs[@]}" claude -p --input-format stream-json --output-format stream-json --verbose \
        --no-session-persistence --debug-file "$log" 2>/dev/null \
    | grep -m1 '"wren-usage"' \
    | sed -E 's/"session":\{[^}]*\{[^}]*\}\},?//; s/,?"behaviors":null//; s/,\}/}/' | cut -c1-1500
  echo "--- Claude Code's own notes about the usage read"
  # Everything about the usage read, plus any error, with secrets masked.
  grep -iE 'usage|utiliz|429|401|403|profile|scope|error|fail|timeout|econn|refresh|oauth' "$log" \
    | sed -E 's/(sk-ant-[A-Za-z0-9_-]{4})[A-Za-z0-9_-]+/\1…/g; s/([Bb]earer )[A-Za-z0-9._~+\/=-]+/\1…/g; s/("?(access|refresh)_?[Tt]oken"?[:=] ?"?)[^",} ]+/\1…/g' \
    | grep -vE 'plugins-scope|GrowthBook' | tail -n 25 | cut -c1-400
  rm -f "$log"
}

check "Default account (your normal claude login)" ""
if [ -d "$WREN_DIR" ]; then
  for d in "$WREN_DIR"/*/; do
    [ -d "$d" ] && check "Wren account folder: $(basename "$d")" "${d%/}"
  done
fi
