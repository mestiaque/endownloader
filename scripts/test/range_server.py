#!/usr/bin/env python3
"""Local HTTP test server with Range support and a per-connection speed limit.

GET /file/<size>/<rate>/<name>   deterministic bytes: b[i] = (i*31 + 7) % 251
GET /status/<code>/<name>        empty reply with that status
Usage: range_server.py <port>
Also: range_server.py --verify <file> <size>   (exit 0 when the content matches)
"""
import sys
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

PERIOD = bytes((i * 31 + 7) % 251 for i in range(251))


def chunk(start, n):
    off = start % 251
    reps = (off + n) // 251 + 1
    return (PERIOD * reps)[off:off + n]


class H(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def log_message(self, *a):
        pass

    def do_HEAD(self):
        self.serve(head=True)

    def do_GET(self):
        self.serve(head=False)

    def serve(self, head):
        seg = self.path.strip("/").split("/")
        if seg[0] == "status":
            self.send_response(int(seg[1]))
            self.send_header("Content-Length", "0")
            self.send_header("Connection", "close")
            self.end_headers()
            return
        size, rate = int(seg[1]), int(seg[2])
        start, end = 0, size - 1
        rng = self.headers.get("Range")
        if rng and rng.startswith("bytes="):
            a, _, b = rng[6:].partition("-")
            start = int(a)
            end = min(int(b), size - 1) if b else size - 1
            if start >= size:
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.end_headers()
                return
            self.send_response(206)
            self.send_header("Content-Range", f"bytes {start}-{end}/{size}")
        else:
            self.send_response(200)
        length = end - start + 1
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Content-Length", str(length))
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Connection", "close")
        self.end_headers()
        if head:
            return
        t0, sent, step = time.monotonic(), 0, 64 * 1024
        try:
            while sent < length:
                n = min(step, length - sent)
                self.wfile.write(chunk(start + sent, n))
                sent += n
                if rate:
                    wait = sent / rate - (time.monotonic() - t0)
                    if wait > 0:
                        time.sleep(wait)
        except (BrokenPipeError, ConnectionResetError):
            pass


def verify(path, size):
    with open(path, "rb") as f:
        pos = 0
        while True:
            b = f.read(1 << 20)
            if not b:
                break
            if b != chunk(pos, len(b)):
                print(f"{path}: content differs near byte {pos}")
                return 1
            pos += len(b)
    if pos != size:
        print(f"{path}: size {pos}, expected {size}")
        return 1
    return 0


if __name__ == "__main__":
    if sys.argv[1] == "--verify":
        sys.exit(verify(sys.argv[2], int(sys.argv[3])))
    ThreadingHTTPServer(("127.0.0.1", int(sys.argv[1])), H).serve_forever()
