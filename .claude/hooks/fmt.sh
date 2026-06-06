#!/usr/bin/env bash
# PostToolUse hook: auto-format Rust files after an agent edits them.
# Receives the tool-call JSON on stdin; we pull out the edited file path.
# Keeps the tree rustfmt-clean so the "Definition of done" gate (and CI) never
# trips on formatting alone. Quality gate is "medium": format here, lint/test in CI.
set -euo pipefail

input="$(cat)"

# Extract .tool_input.file_path without requiring jq.
file="$(printf '%s' "$input" | sed -n 's/.*"file_path"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n1)"

# Only format Rust source files that exist.
case "$file" in
  *.rs)
    if [ -f "$file" ] && command -v rustfmt >/dev/null 2>&1; then
      rustfmt --edition 2021 "$file" 2>/dev/null || true
    fi
    ;;
esac

exit 0
