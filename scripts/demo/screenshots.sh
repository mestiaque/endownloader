#!/usr/bin/env bash
# User-guide screenshots with demo data (run inside the build container; see docs/USER_GUIDE.md).
# Running downloads come from the local Range test server, named like real mirrors through
# --add-host; finished, paused and failed ones are written into the database. The media
# examples are Blender Foundation open movies (Creative Commons).
#
#   ENDL_DOCKER_ARGS="--add-host releases.ubuntu.com:127.0.0.1 --add-host download.blender.org:127.0.0.1 \
#     --add-host cdimage.debian.org:127.0.0.1 -v <yt-dlp_linux>:/usr/local/bin/yt-dlp:ro" \
#     scripts/docker/run.sh "dbus-run-session -- scripts/demo/screenshots.sh"
set -uo pipefail
OUT="${1:-/src/docs/screenshots}"
TMP=/tmp/demo
rm -rf /tmp/dhome "$TMP" && mkdir -p "$OUT" "$TMP"
export HOME=/tmp/dhome XDG_RUNTIME_DIR=/tmp/dhome/run XDG_DATA_HOME=/tmp/dhome/.local/share XDG_CONFIG_HOME=/tmp/dhome/.config
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/Downloads" && chmod 700 "$XDG_RUNTIME_DIR"
DB="$XDG_DATA_HOME/com.mestiaque.ensave/ensave.db"
BIN=/src/target/debug/ensave
PORT=8080
trap 'pkill -f ensave; pkill -f range_server; pkill -f "Xvfb :98"; pkill -f yt-dlp' EXIT

python3 /src/scripts/test/range_server.py $PORT >/dev/null 2>&1 &
Xvfb :98 -screen 0 1280x800x24 >/dev/null 2>&1 &
export DISPLAY=:98
sleep 1
shot() { sleep "${2:-1}"; import -window "${WID:-root}" "$OUT/$1.png"; echo "shot $1"; }   # just the window

# 1. First start creates the database; then add the finished/paused/failed history.
"$BIN" --minimized >"$TMP/app.log" 2>&1 &
for _ in $(seq 1 30); do [ -s "$DB" ] && break; sleep 0.5; done
sleep 2
pkill -f "$BIN"; sleep 1.5
python3 - "$DB" <<'EOF'
import json, sqlite3, sys, time
c = sqlite3.connect(sys.argv[1])
now = int(time.time() * 1000)
dl = "/tmp/dhome/Downloads"
yt = lambda i: f"https://i.ytimg.com/vi/{i}/hqdefault.jpg"
rows = [
    # url, status, title, filename, category, total, done, error, minutes ago, options
    ("https://www.youtube.com/watch?v=aqz-KE-bpKQ", "completed", "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film", "Big Buck Bunny 60fps 4K - Official Blender Foundation Short Film [aqz-KE-bpKQ].mp4", "video", 263_300_000, 263_300_000, None, 95, {"thumbnail": yt("aqz-KE-bpKQ")}),
    ("https://www.youtube.com/watch?v=eRsGyueVLvQ", "completed", "Sintel - Open Movie by Blender Foundation", "Sintel - Open Movie by Blender Foundation [eRsGyueVLvQ].mp4", "video", 151_800_000, 151_800_000, None, 80, {"thumbnail": yt("eRsGyueVLvQ")}),
    ("https://www.youtube.com/watch?v=R6MlUcmOul8", "paused", "Tears of Steel - Blender VFX Open Movie", "Tears of Steel - Blender VFX Open Movie [R6MlUcmOul8].mp4", "video", 412_600_000, 171_200_000, None, 40, {"thumbnail": yt("R6MlUcmOul8")}),
    ("https://www.youtube.com/watch?v=mN0zPOpADL4", "completed", "Agent 327: Operation Barbershop", "Agent 327 - Operation Barbershop [mN0zPOpADL4].mp3", "audio", 5_640_000, 5_640_000, None, 30, {"thumbnail": yt("mN0zPOpADL4")}),
    ("https://www.python.org/ftp/python/3.13.1/python-3.13.1-amd64.exe", "completed", None, "python-3.13.1-amd64.exe", "programs", 27_400_000, 27_400_000, None, 120, None),
    ("https://www.gutenberg.org/ebooks/1342.epub3.images", "completed", None, "pride-and-prejudice.epub", "documents", 24_900_000, 24_900_000, None, 150, None),
    ("https://github.com/git-for-windows/git/releases/download/v2.47.1.windows.1/PortableGit-2.47.1-64-bit.7z.exe", "failed", None, "PortableGit-2.47.1-64-bit.7z.exe", "programs", None, 0, "The site is limiting requests (429). Try again later", 12, None),
    ("https://example.com/reports/annual-report-2025.pdf", "failed", None, "annual-report-2025.pdf", "documents", None, 0, "The file was not found on the server (404)", 8, None),
]
for i, (url, st, title, fn, cat, total, done, err, ago, opt) in enumerate(rows):
    t = now - ago * 60_000
    c.execute(
        "INSERT INTO tasks (url, dedupe_key, engine, status, title, filename, folder, category, total, done, error, position, created_at, finished_at, options) "
        "VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)",
        (url, url, "media" if "youtube" in url else "aria2", st, title, fn, dl, cat, total, done, err, i + 1, t,
         t + 60_000 if st == "completed" else None, json.dumps(opt)),
    )
# The guide uses the Glass theme.
c.execute("INSERT OR REPLACE INTO settings(key, value) VALUES ('theme', '\"glass\"')")
c.commit()
EOF

# 2. Start again with a media link (analysis card), then hand over file links like a browser would.
"$BIN" "https://www.youtube.com/watch?v=aqz-KE-bpKQ" >>"$TMP/app.log" 2>&1 &
sleep 4
B="http://releases.ubuntu.com:$PORT/file"
timeout 10 "$BIN" \
  "$B/6203355136/2500000/ubuntu-24.04.1-desktop-amd64.iso" \
  "http://download.blender.org:$PORT/file/398458880/1800000/blender-4.3.2-linux-x64.tar.xz" \
  "http://cdimage.debian.org:$PORT/file/661651456/1200000/debian-12.8.0-amd64-netinst.iso" \
  "http://download.blender.org:$PORT/file/48234496/900000/blender-4.3.2-windows-x64.msi" \
  "http://releases.ubuntu.com:$PORT/file/2779774976/2500000/ubuntu-24.04.1-live-server-amd64.iso"
for _ in $(seq 1 40); do grep -q "analyzed" "$TMP/app.log" && break; sleep 0.5; done
WID=$(xdotool search --onlyvisible --name ENSave | head -1)
eval "$(xdotool getwindowgeometry --shell "$WID")"
shot 02-analyze 6   # after the "Added 5 downloads" toast is gone

# 3. Close the card: the download list with running, queued, paused, finished and failed tasks.
xdotool mousemove $((X + WIDTH - 41)) $((Y + 89)) click 1
xdotool mousemove $((X + WIDTH / 2)) $((Y + HEIGHT - 30)); sleep 0.5
xdotool key Escape   # clear the selection of the links just added
shot 01-downloads 3
xdotool mousemove $((X + 80)) $((Y + 130)) click 1   # sidebar: Active
xdotool mousemove $((X + WIDTH / 2)) $((Y + HEIGHT - 30))
shot 03-active 2
xdotool mousemove $((X + 80)) $((Y + 96)) click 1    # back to All downloads

# 4. Settings (Ctrl+,), sections by their place in the dialog's left column.
xdotool mousemove $((X + WIDTH / 2)) $((Y + HEIGHT - 30)) click 1; sleep 0.3
xdotool key ctrl+comma
shot 04-settings-general 1.5
section() { xdotool mousemove $((X + 240)) $((Y + $1)) click 1; xdotool mousemove_relative 400 0; shot "$2" "${3:-1.5}"; }
section 164 05-settings-downloads
section 232 06-settings-media
section 266 07-settings-tools 6   # tool versions take a moment
xdotool key Escape
echo "--- app log ---"; tail -15 "$TMP/app.log"
