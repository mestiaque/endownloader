#!/usr/bin/env bash
# Phase 2 acceptance check on the real app (run inside the build container):
# 5 parallel big downloads → kill -9 the app mid-way → aria2c must exit too → restart →
# every download resumes from its partial data and finishes byte-exact.
set -uo pipefail
OUT="${1:-/src/target/phase2}"
SIZE=$((150 * 1024 * 1024))
RATE=$((1024 * 1024))   # per connection; 8 connections → ~8 MB/s per file (~19 s)
PORT=8765
rm -rf "$OUT" /tmp/p2home && mkdir -p "$OUT"
export HOME=/tmp/p2home XDG_RUNTIME_DIR=/tmp/p2home/run XDG_DATA_HOME=/tmp/p2home/.local/share XDG_CONFIG_HOME=/tmp/p2home/.config
mkdir -p "$XDG_RUNTIME_DIR" "$HOME/Downloads" && chmod 700 "$XDG_RUNTIME_DIR"
DB="$XDG_DATA_HOME/com.mestiaque.endownloader/endownloader.db"
DL="$HOME/Downloads/big"
BIN=/src/target/debug/endownloader
SRV=/src/scripts/test/range_server.py

Xvfb :98 -screen 0 1280x720x24 >/dev/null 2>&1 &
export DISPLAY=:98
python3 "$SRV" $PORT &
SRVPID=$!
APP=
# Never leave Xvfb, the server or the app behind (they would keep the caller's pipe open).
trap 'kill $APP $SRVPID 2>/dev/null; pkill -f "Xvfb :98" 2>/dev/null; pkill -x aria2c 2>/dev/null' EXIT
sleep 1

q() { python3 - "$DB" "$1" <<'EOF'
import sqlite3, sys
c = sqlite3.connect(sys.argv[1], timeout=5)
for row in c.execute(sys.argv[2]):
    print(*row)
c.commit()
EOF
}
wait_for() { # seconds, description, shell condition
  local t=0
  until eval "$3"; do sleep 1; t=$((t + 1)); [ $t -ge "$1" ] && { echo "TIMEOUT: $2"; return 1; }; done
  echo "ok: $2 (${t}s)"
}

# First launch creates the database; set 5 downloads at a time.
"$BIN" >"$OUT/run0.log" 2>&1 &
sleep 4; kill $! 2>/dev/null; sleep 1
q "INSERT INTO settings(key,value) VALUES ('maxParallel','5') ON CONFLICT(key) DO UPDATE SET value='5'" >/dev/null

URLS=()
for i in 1 2 3 4 5; do URLS+=("http://127.0.0.1:$PORT/file/$SIZE/$RATE/disk$i.iso"); done
"$BIN" "${URLS[@]}" --folder "$DL" >"$OUT/run1.log" 2>&1 &
APP=$!
wait_for 30 "5 downloads active" '[ "$(q "SELECT COUNT(*) FROM tasks WHERE status='"'"'active'"'"'")" = 5 ]' || exit 1
sleep 3
import -window root "$OUT/1-downloading.png"
pgrep -a aria2c | sed 's/--rpc-secret=[^ ]*/--rpc-secret=***/' | cut -c1-160
ARIA=$(pgrep aria2c)
echo "--- kill -9 the app (pid $APP) ---"
kill -9 $APP
wait_for 10 "aria2c exits after the app died" '! kill -0 $ARIA 2>/dev/null'
echo "partial files at the crash:"
ls -l "$DL" | awk 'NR>1 {print "  " $5, $9}'
q "SELECT id, status, done FROM tasks ORDER BY id" | sed 's/^/  db: /'
BEFORE=$(du -cb "$DL"/*.iso | tail -1 | cut -f1)

echo "--- restart ---"
"$BIN" >"$OUT/run2.log" 2>&1 &
APP=$!
wait_for 20 "5 downloads active again" '[ "$(q "SELECT COUNT(*) FROM tasks WHERE status='"'"'active'"'"'")" = 5 ]' || exit 1
sleep 1
q "SELECT id, status, done FROM tasks ORDER BY id" | sed 's/^/  db: /'
import -window root "$OUT/2-resumed.png"
wait_for 180 "all 5 completed" '[ "$(q "SELECT COUNT(*) FROM tasks WHERE status='"'"'completed'"'"'")" = 5 ]' || exit 1
import -window root "$OUT/3-completed.png"
FAIL=0
for i in 1 2 3 4 5; do
  if python3 "$SRV" --verify "$DL/disk$i.iso" $SIZE; then echo "ok: disk$i.iso byte-exact"; else FAIL=1; fi
  [ -e "$DL/disk$i.iso.aria2" ] && { echo "leftover control file disk$i.iso.aria2"; FAIL=1; }
done
grep -E "returned to the queue|aria2c .* ready|completed|ERROR" "$OUT/run2.log" | sed 's/^/  log: /' | head -12
echo "bytes on disk at crash: $BEFORE of $((5 * SIZE))"
kill $APP $SRVPID 2>/dev/null
[ $FAIL = 0 ] && echo "PHASE 2 CHECK: PASS" || echo "PHASE 2 CHECK: FAIL"
