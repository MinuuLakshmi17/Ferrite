#!/usr/bin/env python3
"""End-to-end test harness for ferrite.

Builds a .torrent for a random file, serves a fake HTTP tracker and a fake
BitTorrent peer, runs `ferrite download`, and verifies the output byte-for-byte.
"""
import hashlib
import http.server
import os
import random
import socket
import struct
import subprocess
import sys
import threading

TRACKER_PORT = 18080
PEER_PORTS = [16881, 16882, 16883]
PIECE_LEN = 32768
FILE_SIZE = 200 * 1024  # 200 KiB -> 7 pieces


def benc(x):
    if isinstance(x, int):
        return b"i" + str(x).encode() + b"e"
    if isinstance(x, bytes):
        return str(len(x)).encode() + b":" + x
    if isinstance(x, list):
        return b"l" + b"".join(benc(i) for i in x) + b"e"
    if isinstance(x, dict):
        out = b"d"
        for k in sorted(x.keys()):
            out += benc(k) + benc(x[k])
        return out + b"e"
    raise TypeError(x)


def main():
    workdir = sys.argv[1]
    os.makedirs(workdir, exist_ok=True)
    data = random.randbytes(FILE_SIZE)
    with open(os.path.join(workdir, "original.bin"), "wb") as f:
        f.write(data)

    pieces = b"".join(
        hashlib.sha1(data[i : i + PIECE_LEN]).digest()
        for i in range(0, len(data), PIECE_LEN)
    )
    npieces = (len(data) + PIECE_LEN - 1) // PIECE_LEN
    info = {
        b"name": b"original.bin",
        b"piece length": PIECE_LEN,
        b"pieces": pieces,
        b"length": len(data),
    }
    info_benc = benc(info)
    info_hash = hashlib.sha1(info_benc).digest()
    torrent = {b"announce": f"http://127.0.0.1:{TRACKER_PORT}/announce".encode(), b"info": info}
    # hand-encode root to embed raw info dict bytes untouched
    root_raw = b"d8:announce" + benc(f"http://127.0.0.1:{TRACKER_PORT}/announce".encode()) + b"4:info" + info_benc + b"e"
    tpath = os.path.join(workdir, "test.torrent")
    with open(tpath, "wb") as f:
        f.write(root_raw)

    # ---- fake tracker ----
    peers_compact = b"".join(socket.inet_aton("127.0.0.1") + struct.pack(">H", p) for p in PEER_PORTS)

    class Handler(http.server.BaseHTTPRequestHandler):
        def do_GET(self):
            body = benc({b"interval": 1800, b"peers": peers_compact})
            self.send_response(200)
            self.send_header("Content-Type", "text/plain")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, *a):
            pass

    tracker = http.server.HTTPServer(("127.0.0.1", TRACKER_PORT), Handler)
    threading.Thread(target=tracker.serve_forever, daemon=True).start()

    # ---- fake peer ----
    def read_n(conn, n):
        buf = b""
        while len(buf) < n:
            chunk = conn.recv(n - len(buf))
            if not chunk:
                raise ConnectionError("peer closed")
            buf += chunk
        return buf

    def send_msg(conn, mid, payload=b""):
        conn.sendall(struct.pack(">I", 1 + len(payload)) + bytes([mid]) + payload)

    def peer_server(port):
        srv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
        srv.bind(("127.0.0.1", port))
        srv.listen(4)
        while True:
            conn, _ = srv.accept()
            threading.Thread(target=handle_peer, args=(conn,), daemon=True).start()

    def handle_peer(conn):
        try:
            hs = read_n(conn, 68)
            assert hs[0] == 19 and hs[1:20] == b"BitTorrent protocol", "bad handshake"
            assert hs[28:48] == info_hash, "info-hash mismatch"
            conn.sendall(
                bytes([19]) + b"BitTorrent protocol" + b"\x00" * 8 + info_hash + b"-PY0001-fakepeer0000"
            )
            # expect interested
            ln = struct.unpack(">I", read_n(conn, 4))[0]
            msg = read_n(conn, ln)
            assert msg[0] == 2, f"expected interested, got {msg[0]}"
            # bitfield: all pieces available
            bf = bytearray((npieces + 7) // 8)
            for i in range(npieces):
                bf[i // 8] |= 0x80 >> (i % 8)
            send_msg(conn, 5, bytes(bf))
            send_msg(conn, 1)  # unchoke
            while True:
                ln = struct.unpack(">I", read_n(conn, 4))[0]
                if ln == 0:
                    continue
                msg = read_n(conn, ln)
                if msg[0] == 6:  # request
                    index, begin, length = struct.unpack(">III", msg[1:13])
                    start = index * PIECE_LEN + begin
                    chunk = data[start : start + length]
                    send_msg(conn, 7, struct.pack(">II", index, begin) + chunk)
        except (ConnectionError, AssertionError, BrokenPipeError):
            pass
        finally:
            conn.close()

    for pt in PEER_PORTS:
        threading.Thread(target=peer_server, args=(pt,), daemon=True).start()

    # ---- run ferrite ----
    outdir = os.path.join(workdir, "out")
    ferrite = sys.argv[2] if len(sys.argv) > 2 else "ferrite"
    r = subprocess.run(
        [ferrite, "download", tpath, "--output", outdir, "--peers", "1"],
        capture_output=True,
        text=True,
        timeout=120,
    )
    print(r.stdout)
    print(r.stderr, file=sys.stderr)

    got_path = os.path.join(outdir, "original.bin")
    if not os.path.exists(got_path):
        print("FAIL: output file missing")
        sys.exit(1)
    with open(got_path, "rb") as f:
        got = f.read()
    if got == data:
        print(f"E2E PASS: {len(got)} bytes, sha256 matches")
    else:
        print(f"FAIL: got {len(got)} bytes, want {len(data)}; match={got == data}")
        sys.exit(1)


if __name__ == "__main__":
    main()
