#!/bin/zsh
set -euo pipefail

SCRIPT_PATH="${(%):-%x}"
SCRIPT_DIR="$(cd "$(dirname "$SCRIPT_PATH")" && pwd)"
DEFAULT_MESSAGE="Deploy mac DMG"

if command -v osascript >/dev/null 2>&1; then
  MESSAGE="$(
    osascript <<'APPLESCRIPT'
try
  set dialogResult to display dialog "Commit message for this deploy:" default answer "Deploy mac DMG" buttons {"Cancel", "Deploy"} default button "Deploy"
  return text returned of dialogResult
on error number -128
  return ""
end try
APPLESCRIPT
  )"
else
  MESSAGE="$DEFAULT_MESSAGE"
fi

if [[ -z "${MESSAGE//[[:space:]]/}" ]]; then
  echo "Deploy canceled: no commit message provided."
  exit 1
fi

cd "$SCRIPT_DIR"
echo "Running macOS deploy from $SCRIPT_DIR"
echo "Commit message: $MESSAGE"
echo ""

npm run deploy:mac -- -m "$MESSAGE"

echo ""
echo "Deploy finished. You can close this window."
