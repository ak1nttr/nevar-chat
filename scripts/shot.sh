#!/usr/bin/env bash
# Renders UI scenes from the dev mock to PNGs with headless Edge (no window appears).
# Usage: scripts/shot.sh <out-dir> [scene...]
#   scenes: home call incoming ringing settings empty update (default: home call incoming settings)
#   WIDTH / HEIGHT env vars set the window size (default 380x660).
#   THEME=light|dark forces a theme; its shots are named <scene>-<theme>.png.
set -uo pipefail

OUT="$1"
shift
SCENES=("$@")
[[ ${#SCENES[@]} -eq 0 ]] && SCENES=(home call incoming settings)
EDGE="/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"

mkdir -p "$OUT"
cd "$(dirname "$0")/../app"
started=0
if ! curl -s -o /dev/null http://localhost:1420/; then
  (pnpm dev > /dev/null 2>&1 &)
  until curl -s -o /dev/null http://localhost:1420/; do sleep 0.5; done
  started=1
fi
# A dev server this script started must not outlive it: it holds port 1420, which `pnpm tauri dev` needs.
stop_server() {
  [[ $started -eq 1 ]] || return 0
  local pid
  pid="$(netstat -ano | awk '/:1420 .*LISTENING/ {print $NF; exit}')"
  [[ -n "$pid" ]] && taskkill //PID "$pid" //T //F > /dev/null 2>&1
}
trap stop_server EXIT

win_out="$(cygpath -w "$OUT")"
profile="$(cygpath -w "${TMPDIR:-/tmp}")\\nevar-shot-profile"
w="${WIDTH:-380}"
h="${HEIGHT:-660}"
# Edge has a minimum window width, so the window may be wider; the app is framed at w x h inside it.
win_w=$((w < 520 ? 520 : w))
for scene in "${SCENES[@]}"; do
  "$EDGE" --headless=new --disable-gpu --hide-scrollbars --force-device-scale-factor=1 \
    --window-size="${win_w},${h}" --virtual-time-budget=2500 \
    --user-data-dir="$profile" \
    --screenshot="${win_out}\\${scene}${THEME:+-$THEME}.png" "http://localhost:1420/?scene=${scene}&frame=${w}x${h}${THEME:+&theme=$THEME}" > /dev/null 2>&1 ||
    echo "failed: $scene" >&2
done
ls "$OUT"
