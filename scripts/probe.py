"""Headless probe for the uvagent wire protocol.

Connects to an agent, receives frames, optionally sends control messages, and
writes the newest frame to disk so it can be inspected without the GUI.

    python native/scripts/probe.py --frames 3 --out frame.jpg
    python native/scripts/probe.py --json '{"t":"login","id":"123","pw":"x"}'
"""

import argparse
import json
import socket
import struct
import sys
import time

KIND_CTL = 1
KIND_FRAME = 2


def recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("agent closed the connection")
        buf += chunk
    return buf


def read_msg(sock):
    header = recv_exact(sock, 5)
    (length,) = struct.unpack("<I", header[:4])
    kind = header[4]
    return kind, recv_exact(sock, length)


def send_msg(sock, kind, payload):
    sock.sendall(struct.pack("<I", len(payload)) + bytes([kind]) + payload)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--host", default="127.0.0.1")
    ap.add_argument("--port", type=int, default=7801)
    ap.add_argument("--frames", type=int, default=2, help="how many frames to collect")
    ap.add_argument("--timeout", type=float, default=15.0)
    ap.add_argument("--out", default="frame.jpg")
    ap.add_argument(
        "--json",
        action="append",
        default=[],
        help="control message to send after a frame (repeatable, sent in order)",
    )
    ap.add_argument(
        "--json-file",
        default=None,
        help="file with one JSON control message per line (shell-quoting safe)",
    )
    args = ap.parse_args()

    if args.json_file:
        with open(args.json_file, "r", encoding="utf-8-sig") as fh:
            args.json.extend(line.strip() for line in fh if line.strip())

    sock = socket.create_connection((args.host, args.port), timeout=args.timeout)
    sock.settimeout(args.timeout)
    print(f"connected to {args.host}:{args.port}")

    frames = 0
    sent = 0
    last = None
    started = time.time()

    while frames < args.frames and time.time() - started < args.timeout:
        try:
            kind, payload = read_msg(sock)
        except (TimeoutError, socket.timeout):
            print("no further frames (window idle) — keeping the last one")
            break
        if kind == KIND_CTL:
            print("event:", json.loads(payload.decode()))
            continue

        if kind == KIND_FRAME:
            w, h, seq = struct.unpack("<HHI", payload[:8])
            jpeg = payload[8:]
            frames += 1
            last = (w, h, seq, jpeg)
            print(f"frame {frames}: {w}x{h} seq={seq} jpeg={len(jpeg):,} bytes")

            # Fire one queued control message per frame, leaving time for the
            # agent to act before we expect new pixels.
            if sent < len(args.json):
                time.sleep(0.35)
                message = args.json[sent]
                send_msg(sock, KIND_CTL, message.encode())
                print("sent:", message)
                sent += 1

    if last is None:
        print("no frame received", file=sys.stderr)
        return 1

    w, h, seq, jpeg = last
    with open(args.out, "wb") as fh:
        fh.write(jpeg)
    print(f"wrote {args.out} ({w}x{h}, seq {seq})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
