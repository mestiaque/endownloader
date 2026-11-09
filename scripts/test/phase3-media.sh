#!/usr/bin/env bash
# Phase 3 check on the real app (inside the build container, with yt-dlp/node mounted from
# the host; see the README). Screenshots go to target/phase3.
#  1. A YouTube link opens the analysis card (title, thumbnail, Video/Audio tables).
#  2. The command line with --quality queues it directly; it downloads and merges to MP4.
set -uo pipefail
OUT="${1:-/src/target/phase3}"
LINK="${LINK:-https://www.youtube.com/watch?v=mk48xRzuNvA&list=RDmk48xRzuNvA&start_radio=1}"
rm -rf "$OUT" /tmp/p3home && mkdir -p "$OUT"
export HOME=/tmp/p3home XDG_RUNTIME_DIR=/tmp/p3home/run XDG_DATA_HOME=/tmp/p3home/.local/share XDG_CONFIG_HOME=/tmp/p3home/.config
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/Downloads" && chmod 700 "$XDG_RUNTIME_DIR"
DB="$XDG_DATA_HOME/com.mestiaque.endownloader/endownloader.db"
BIN=/src/target/debug/endownloader
APP=
trap 'kill $APP 2>/dev/null; pkill -f "Xvfb :97" 2>/dev/null; pkill -f yt-dlp 2>/dev/null' EXIT
Xvfb :97 -screen 0 1280x720x24 >/dev/null 2>&1 &
export DISPLAY=:97
sleep 1
q() { python3 - "$DB" "$1" <<'EOF'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1], timeout=5)
for row in c.execute(sys.argv[2]):
    print(*row)
c.commit()
EOF
}
echo "tools on PATH: yt-dlp=$(command -v yt-dlp) node=$(command -v node) ffmpeg=$(command -v ffmpeg)"

# 1. Analysis card.
"$BIN" "$LINK" >"$OUT/app.log" 2>&1 &
APP=$!
for i in $(seq 1 40); do grep -q "analyzed" "$OUT/app.log" && break; sleep 1; done
grep -E "analyzed|analyze failed" "$OUT/app.log" | sed 's/^/  log: /'
sleep 3
import -window root "$OUT/1-card.png"

# 2. Second launch with a quality: queued directly as a media task.
timeout 10 "$BIN" "$LINK" --quality 480
for i in $(seq 1 120); do
  st=$(q "SELECT status FROM tasks WHERE engine='media' ORDER BY id DESC LIMIT 1")
  [ "$st" = active ] && [ ! -e "$OUT/2-downloading.png" ] && { sleep 2; import -window root "$OUT/2-downloading.png"; }
  [ "$st" = completed ] || [ "$st" = failed ] && break
  sleep 1
done
q "SELECT id, status, title, filename, done, total, error FROM tasks" | sed 's/^/  db: /'
sleep 1   # let the UI receive the final update and finish the progress-bar transition
import -window root "$OUT/3-done.png"
ls -la "$HOME/Downloads/Video" 2>&1 | sed 's/^/  /'
F=$(ls "$HOME"/Downloads/Video/*.mp4 "$HOME"/Downloads/Video/*.mkv 2>/dev/null | head -1)
if [ -n "$F" ]; then
  ffprobe -v error -show_entries stream=codec_type,codec_name,height -of csv=p=0 "$F" | sed 's/^/  stream: /'
  echo "PHASE 3 MEDIA CHECK: PASS"
else
  echo "PHASE 3 MEDIA CHECK: FAIL"; tail -20 "$OUT/app.log"
fi
