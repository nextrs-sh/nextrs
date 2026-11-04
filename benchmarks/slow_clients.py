#!/usr/bin/env python3
"""Server RSS while N clients pause in the middle of large streamed responses.

Opens N connections to a running server, requests SIZE bytes from the
fixture's /api/stream route on each, reads only the response head, waits,
then samples the server's RSS (from /proc/<pid>/status). Afterwards it reads
every response to the end and checks its length.

    python3 benchmarks/slow_clients.py --pid <server pid> --port 3001 \
        --clients 1,8,32,128 --size-mib 32 [--json out.json]

Linux only (reads /proc). Standard library only.
"""

import argparse
import json
import socket
import time


def rss_kib(pid):
    with open(f"/proc/{pid}/status") as status:
        for line in status:
            if line.startswith("VmRSS:"):
                return int(line.split()[1])
    raise RuntimeError("no VmRSS")


def open_paused(port, size):
    sock = socket.create_connection(("127.0.0.1", port))
    # A small receive buffer, so the kernel absorbs little of the response.
    sock.setsockopt(socket.SOL_SOCKET, socket.SO_RCVBUF, 64 * 1024)
    sock.sendall(f"GET /api/stream?size={size} HTTP/1.1\r\nhost: x\r\n\r\n".encode())
    head = b""
    while b"\r\n\r\n" not in head:
        part = sock.recv(1)
        if not part:
            raise RuntimeError("closed before head")
        head += part
    status = int(head.split(b" ", 2)[1])
    if status != 200:
        raise RuntimeError(f"status {status}")
    return sock


def read_chunked(sock):
    """Reads a chunked body to its end; returns the body length."""
    buf = b""
    total = 0
    while True:
        while b"\r\n" not in buf:
            more = sock.recv(65536)
            if not more:
                raise RuntimeError("EOF in chunk size")
            buf += more
        line, buf = buf.split(b"\r\n", 1)
        size = int(line.split(b";")[0], 16)
        while len(buf) < size + 2:
            more = sock.recv(max(65536, size + 2 - len(buf)))
            if not more:
                raise RuntimeError("EOF in chunk")
            buf += more
        buf = buf[size + 2 :]
        if size == 0:
            return total
        total += size


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--pid", type=int, required=True)
    parser.add_argument("--port", type=int, default=3001)
    parser.add_argument("--clients", default="1,8,32,128")
    parser.add_argument("--size-mib", type=int, default=32)
    parser.add_argument("--pause", type=float, default=3.0)
    parser.add_argument("--json")
    args = parser.parse_args()
    size = args.size_mib * 1024 * 1024
    results = []
    for n in [int(x) for x in args.clients.split(",")]:
        time.sleep(1.0)
        before = rss_kib(args.pid)
        socks = [open_paused(args.port, size) for _ in range(n)]
        time.sleep(args.pause)
        paused = rss_kib(args.pid)
        started = time.monotonic()
        lengths = [read_chunked(s) for s in socks]
        drain_s = time.monotonic() - started
        for s in socks:
            s.close()
        time.sleep(1.0)
        after = rss_kib(args.pid)
        complete = all(length == size for length in lengths)
        row = {
            "clients": n,
            "paused_payload_mib": n * args.size_mib,
            "rss_before_mib": round(before / 1024, 1),
            "rss_paused_mib": round(paused / 1024, 1),
            "growth_mib": round((paused - before) / 1024, 1),
            "growth_per_client_kib": round((paused - before) / n, 1),
            "rss_after_mib": round(after / 1024, 1),
            "all_complete": complete,
            "drain_seconds": round(drain_s, 2),
        }
        print(json.dumps(row))
        results.append(row)
    if args.json:
        with open(args.json, "w") as out:
            json.dump({"size_mib": args.size_mib, "pause_s": args.pause, "runs": results}, out, indent=2)


if __name__ == "__main__":
    main()
