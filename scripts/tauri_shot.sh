#!/bin/sh
# Start the Tauri shell on a virtual display, wait for it to draw, then take one
# screenshot of the screen. Used to look at the window in a machine that has no
# display of its own.
#
#   scripts/tauri_shot.sh <out.png> [document.ofd]
set -e

OUT="${1:?usage: tauri_shot.sh <out.png> [document]}"
DOC="${2:-}"
DISPLAY_NUM=":${SOFD_SHOT_DISPLAY:-91}"
BIN="$(dirname "$0")/../target/debug/sofd-tauri"

if [ ! -x "$BIN" ]; then
  # Building rather than failing, because the binary is what this script is
  # for and a caller who has not built yet has nothing else to do.
  cargo build -p sofd-tauri --manifest-path "$(dirname "$0")/../Cargo.toml" >&2
fi

# WebKitGTK composites through DMABUF when it can, and a machine with no GPU
# has nothing to composite through: the window appears, the stylesheet applies
# and nothing paints. Disabling the compositing mode is what makes a screenshot
# of a headless run show what a reader would see.
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export LIBGL_ALWAYS_SOFTWARE=1

Xvfb "$DISPLAY_NUM" -screen 0 1200x900x24 >/tmp/tauri-shot-xvfb.log 2>&1 &
XVFB=$!
sleep 2

if [ -n "$DOC" ]; then
  DISPLAY="$DISPLAY_NUM" "$BIN" "$DOC" >/tmp/tauri-shot-app.log 2>&1 &
else
  DISPLAY="$DISPLAY_NUM" "$BIN" >/tmp/tauri-shot-app.log 2>&1 &
fi
APP=$!

# WebKitGTK lays out, loads the script, answers a few IPC calls and only then
# draws a page; ten seconds is well past the last of those on software
# rendering, and the shot is taken after that rather than at a guess.
sleep 10
DISPLAY="$DISPLAY_NUM" import -window root "$OUT"

# The pids this run started, rather than a pattern: `pkill -f sofd-tauri` also
# matches the command line of a shell that mentions the name, caller included.
kill "$APP" 2>/dev/null || true
kill "$XVFB" 2>/dev/null || true
wait "$APP" 2>/dev/null || true
echo "wrote $OUT"
