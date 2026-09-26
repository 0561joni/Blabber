#!/bin/zsh
# Diagnostic: shows how Blabber is signed and whether the identity is stable.
cd "$(dirname "$0")"
OUT="Claude outputs/signing-diagnostic.txt"
{
  echo "== date"; date
  echo "== codesigning identities (valid)"; security find-identity -v -p codesigning
  echo "== codesigning identities (all)"; security find-identity -p codesigning
  echo "== certificate"; security find-certificate -c "Blabber Local Signing" -Z 2>&1 | head -5
  echo "== trust settings"; security dump-trust-settings 2>&1 | grep -A3 "Blabber" || echo "(no user trust setting)"
  echo "== installed copies"; mdfind "kMDItemCFBundleIdentifier == 'com.jonibuch.speechtotext'"
  for APP in /Applications/Blabber.app "src-tauri/target/release/bundle/macos/Blabber.app"; do
    echo "== $APP"
    ls -ld "$APP" 2>&1
    codesign -dvv "$APP" 2>&1 | grep -E "Identifier|Authority|Signature|TeamIdentifier|CDHash"
    echo "-- designated requirement"; codesign -d -r- "$APP" 2>&1 | grep designated
    echo "-- verify"; codesign --verify --deep --strict "$APP" 2>&1 && echo ok
    echo "-- quarantine"; xattr -p com.apple.quarantine "$APP" 2>&1
  done
  echo "== running"; pgrep -fl speech-to-text
} > "$OUT" 2>&1
echo "Saved to $OUT"
