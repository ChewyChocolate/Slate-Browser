#!/usr/bin/env bash
# Checks whether Slate's pinned `cef` crate and Chromium engine version are up to date
# with:
#   1. The latest `cef` crate published on crates.io
#   2. The latest upstream Google Chromium Stable release for Linux (via ChromiumDash API)

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

LOCK_LINE=$(awk '/^name = "cef-dll-sys"/{getline; print $0}' Cargo.lock | tr -d '"' | awk '{print $3}')
PINNED_CRATE_VER="${LOCK_LINE%%+*}"
PINNED_CEF_VER="${LOCK_LINE#*+}"
PINNED_CHROMIUM_MAJOR="${PINNED_CRATE_VER%%.*}"

echo "==> Slate Pinned Engine Version:"
echo "    cef crate version:      $PINNED_CRATE_VER"
echo "    CEF binary build:       $PINNED_CEF_VER"
echo "    Chromium major milestone: M$PINNED_CHROMIUM_MAJOR"

LATEST_CRATE_JSON=$(curl -fsSL -A "Slate-Browser-Updater/0.1" "https://crates.io/api/v1/crates/cef" 2>/dev/null || echo "")
if [ -n "$LATEST_CRATE_JSON" ]; then
    LATEST_CRATE_VER=$(python3 -c 'import sys, json; data=json.load(sys.stdin); print(data["crate"]["max_stable_version"])' <<< "$LATEST_CRATE_JSON" 2>/dev/null || echo "unknown")
else
    LATEST_CRATE_VER="offline"
fi

CHROMIUM_DASH_JSON=$(curl -fsSL "https://chromiumdash.appspot.com/fetch_releases?channel=Stable&platform=Linux&num=1" 2>/dev/null || echo "")
if [ -n "$CHROMIUM_DASH_JSON" ]; then
    LATEST_CHROMIUM_FULL=$(python3 -c 'import sys, json; data=json.load(sys.stdin); print(data[0]["version"])' <<< "$CHROMIUM_DASH_JSON" 2>/dev/null || echo "unknown")
    LATEST_CHROMIUM_MAJOR="${LATEST_CHROMIUM_FULL%%.*}"
else
    LATEST_CHROMIUM_FULL="offline"
    LATEST_CHROMIUM_MAJOR="offline"
fi

echo "==> Upstream Versions:"
echo "    crates.io latest cef:   $LATEST_CRATE_VER"
echo "    Chromium Linux Stable:  $LATEST_CHROMIUM_FULL (M$LATEST_CHROMIUM_MAJOR)"

STATUS="CURRENT"
BEHIND_REASONS=()

LATEST_CRATE_BASE="${LATEST_CRATE_VER%%+*}"

if [ "$LATEST_CRATE_BASE" != "offline" ] && [ "$LATEST_CRATE_BASE" != "unknown" ]; then
    if [ "$PINNED_CRATE_VER" != "$LATEST_CRATE_BASE" ]; then
        STATUS="BEHIND"
        BEHIND_REASONS+=("crates.io has newer cef crate ($LATEST_CRATE_VER > $LOCK_LINE)")
    fi
fi

if [ "$LATEST_CHROMIUM_MAJOR" != "offline" ] && [ "$LATEST_CHROMIUM_MAJOR" != "unknown" ]; then
    if [ "$PINNED_CHROMIUM_MAJOR" -lt "$LATEST_CHROMIUM_MAJOR" ] 2>/dev/null; then
        STATUS="BEHIND"
        BEHIND_REASONS+=("Chromium Linux Stable is on M$LATEST_CHROMIUM_MAJOR ($LATEST_CHROMIUM_FULL) while Slate is on M$PINNED_CHROMIUM_MAJOR")
    fi
fi

if [ "$STATUS" = "CURRENT" ]; then
    echo "==> STATUS: UP TO DATE (Slate is on the latest available CEF $PINNED_CRATE_VER / Chromium M$PINNED_CHROMIUM_MAJOR)"
    exit 0
else
    echo "==> STATUS: UPDATE AVAILABLE"
    for r in "${BEHIND_REASONS[@]}"; do
        echo "    - $r"
    done
    echo "    Run: cargo update -p cef -p cef-dll-sys && ./scripts/check.sh"
    exit 2
fi
