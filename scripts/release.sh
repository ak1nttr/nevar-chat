#!/usr/bin/env bash
# Builds, signs and publishes a release that installed copies pick up as an update.
#
# Usage (Git Bash, from anywhere):  scripts/release.sh "What changed, in Turkish"
# Bump "version" in app/src-tauri/tauri.conf.json first and commit.
#
# Needs: gh logged in, and the updater signing key at ~/.tauri/nevar-updater.key
# (never commit it; losing it means installed copies can't be updated anymore).

set -euo pipefail

REPO="ak1nttr/nevar-chat"
KEY="$HOME/.tauri/nevar-updater.key"
NOTES="${1:-}"

cd "$(dirname "$0")/.."

if [[ -z "$NOTES" ]]; then
  echo "usage: scripts/release.sh \"release notes\"" >&2
  exit 1
fi
if [[ ! -f "$KEY" ]]; then
  echo "signing key not found at $KEY" >&2
  exit 1
fi
if [[ -n "$(git status --porcelain)" ]]; then
  echo "commit your changes first, so the release matches a commit" >&2
  exit 1
fi

VERSION=$(node -p "require('./app/src-tauri/tauri.conf.json').version")
TAG="v$VERSION"
if gh release view "$TAG" --repo "$REPO" >/dev/null 2>&1; then
  echo "$TAG is already released; bump the version first" >&2
  exit 1
fi

echo "building $TAG..."
(
  cd app
  TAURI_SIGNING_PRIVATE_KEY="$KEY" TAURI_SIGNING_PRIVATE_KEY_PASSWORD="" pnpm tauri build
)

BUNDLE="target/release/bundle/nsis"
INSTALLER="nevar_${VERSION}_x64-setup.exe"
OUT="dist/$TAG"
mkdir -p "$OUT"
cp "$BUNDLE/$INSTALLER" "$OUT/"

# The manifest the app's updater reads from releases/latest/download/latest.json.
NOTES="$NOTES" VERSION="$VERSION" SIG_FILE="$BUNDLE/$INSTALLER.sig" \
URL="https://github.com/$REPO/releases/download/$TAG/$INSTALLER" node -e '
  const fs = require("fs");
  const manifest = {
    version: process.env.VERSION,
    notes: process.env.NOTES,
    pub_date: new Date().toISOString(),
    platforms: {
      "windows-x86_64": {
        signature: fs.readFileSync(process.env.SIG_FILE, "utf8").trim(),
        url: process.env.URL,
      },
    },
  };
  fs.writeFileSync(process.argv[1], JSON.stringify(manifest, null, 2));
' "$OUT/latest.json"

git tag "$TAG"
git push origin "$TAG"
gh release create "$TAG" "$OUT/$INSTALLER" "$OUT/latest.json" \
  --repo "$REPO" --title "nevar $VERSION" --notes "$NOTES"

echo "released $TAG: https://github.com/$REPO/releases/tag/$TAG"
