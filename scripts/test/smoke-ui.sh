#!/usr/bin/env bash
# Headless smoke test (run inside the build container): start the app under Xvfb with a link
# on the command line, send more links from a second launch, take screenshots.
set -uo pipefail
OUT="${1:-/src/target/smoke}"
mkdir -p "$OUT"
export HOME=/tmp/home XDG_RUNTIME_DIR=/tmp/xdg-run XDG_DATA_HOME=/tmp/home/.local/share XDG_CONFIG_HOME=/tmp/home/.config
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/Downloads" && chmod 700 "$XDG_RUNTIME_DIR"
Xvfb :99 -screen 0 1280x720x24 >/dev/null 2>&1 &
export DISPLAY=:99
sleep 1
BIN=/src/target/debug/endownloader
"$BIN" "https://releases.ubuntu.com/24.04/ubuntu-24.04.1-desktop-amd64.iso" --folder /tmp/home/Downloads/isos >"$OUT/app.log" 2>&1 &
APP=$!
sleep 6
# Second launch: must hand its links to the first instance and exit.
timeout 10 "$BIN" "example.com/files/report.pdf" "https://youtu.be/dQw4w9WgXcQ" --quality 1080
echo "second instance exit code: $?"
sleep 2
import -window root "$OUT/main.png"
xdotool search --name ENDownloader | head -1 > "$OUT/wid.txt"
# Open settings with Ctrl+,
WID=$(cat "$OUT/wid.txt")
eval "$(xdotool getwindowgeometry --shell "$WID")"
xdotool mousemove $((X + WIDTH / 2)) $((Y + HEIGHT - 120)) click 1   # empty list area → focus
sleep 0.3
xdotool key ctrl+comma
sleep 1.5
import -window root "$OUT/settings.png"
# Settings sections, located by their position in the dialog's left column.
xdotool mousemove $((X + 240)) $((Y + 156)) click 1; sleep 1; xdotool mousemove_relative 3 3; sleep 1; import -window root "$OUT/settings-downloads.png"
xdotool mousemove $((X + 240)) $((Y + 190)) click 1; sleep 1; xdotool mousemove_relative 3 3; sleep 1; import -window root "$OUT/settings-categories.png"
xdotool mousemove $((X + 240)) $((Y + 224)) click 1; sleep 1; xdotool mousemove_relative 3 3; sleep 1; import -window root "$OUT/settings-appearance.png"
xdotool mousemove $((X + 600)) $((Y + 180)) click 1; sleep 1; xdotool mousemove_relative 3 3; sleep 1; xdotool key Escape; sleep 1; import -window root "$OUT/light.png"   # "Light" theme card
xdotool key Escape
sleep 0.5
# Close button with close-to-tray on: the process must keep running.
xdotool windowclose "$WID" 2>/dev/null || xdotool key --window "$WID" alt+F4
sleep 1.5
if kill -0 $APP 2>/dev/null; then echo "still running after window close: yes"; else echo "still running after window close: NO"; fi
kill $APP 2>/dev/null; sleep 1
echo "--- app log ---"; cat "$OUT/app.log" | tail -20
echo "--- db ---"
ls -la /tmp/home/.local/share/com.mestiaque.endownloader/ 2>&1
