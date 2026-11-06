#!/usr/bin/env python3
"""The browser client on ONE address: the page and its relay behind one port.

Usage: serve_site.py [--port 8090] [--root client/dist] [--relay 127.0.0.1:2290]

A request asking for a WebSocket (Upgrade: websocket) is passed as it is to the
relay (crates/relay, run beside it); every other one is a GET/HEAD of a file
under --root. The loader page (client/web/index.html) uses its own address as
the relay when the link names none, so a tunnel to this one port
(cloudflared tunnel --url http://127.0.0.1:8090) is the whole site.

Files are served with keep-alive; the page, the scripts and the wasm are
revalidated on every load (a new build is never hidden by a cached one), the
game data may be cached for an hour. Symlinks under the root (assets/) are
followed: the root is what is published.
"""
import argparse
import asyncio
import email.utils
import mimetypes
import os
import urllib.parse
from pathlib import Path

mimetypes.add_type("application/wasm", ".wasm")
mimetypes.add_type("text/javascript", ".js")
mimetypes.add_type("model/gltf-binary", ".glb")
mimetypes.add_type("image/ktx2", ".ktx2")

FRESH = {".html", ".js", ".wasm"}
CHUNK = 1 << 18


async def read_head(reader):
    try:
        head = await reader.readuntil(b"\r\n\r\n")
    except (asyncio.IncompleteReadError, asyncio.LimitOverrunError, ConnectionError):
        return None
    return head


def parse(head):
    lines = head.decode("latin-1").split("\r\n")
    method, target, _version = (lines[0].split(" ") + ["", ""])[:3]
    headers = {}
    for line in lines[1:]:
        if ":" in line:
            k, v = line.split(":", 1)
            headers[k.strip().lower()] = v.strip()
    return method, target, headers


async def pipe(reader, writer):
    try:
        while data := await reader.read(CHUNK):
            writer.write(data)
            await writer.drain()
    except (ConnectionError, asyncio.CancelledError):
        pass
    finally:
        try:
            writer.close()
        except Exception:
            pass


async def to_relay(head, reader, writer, relay):
    host, port = relay.rsplit(":", 1)
    try:
        r_reader, r_writer = await asyncio.open_connection(host, int(port))
    except OSError:
        writer.write(b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
        await writer.drain()
        writer.close()
        return
    r_writer.write(head)
    await r_writer.drain()
    await asyncio.gather(pipe(reader, r_writer), pipe(r_reader, writer))


def simple(writer, status, text, keep):
    body = text.encode()
    writer.write((f"HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\n"
                  f"Content-Length: {len(body)}\r\nConnection: {'keep-alive' if keep else 'close'}\r\n\r\n").encode() + body)


async def serve_file(method, target, headers, writer, root):
    path = urllib.parse.unquote(urllib.parse.urlsplit(target).path)
    parts = [p for p in path.split("/") if p not in ("", ".")]
    keep = headers.get("connection", "").lower() != "close"
    if method not in ("GET", "HEAD"):
        simple(writer, "405 Method Not Allowed", "method not allowed", keep)
        return keep
    if ".." in parts:
        simple(writer, "403 Forbidden", "forbidden", keep)
        return keep
    file = root.joinpath(*parts) if parts else root
    if file.is_dir():
        file = file / "index.html"
    if not file.is_file():
        simple(writer, "404 Not Found", "not found", keep)
        return keep
    stat = file.stat()
    kind = mimetypes.guess_type(file.name)[0] or "application/octet-stream"
    cache = "no-cache" if file.suffix in FRESH else "public, max-age=3600"
    modified = email.utils.formatdate(stat.st_mtime, usegmt=True)
    if headers.get("if-modified-since") == modified:
        writer.write((f"HTTP/1.1 304 Not Modified\r\nCache-Control: {cache}\r\nLast-Modified: {modified}\r\n"
                      f"Connection: {'keep-alive' if keep else 'close'}\r\n\r\n").encode())
        return keep
    writer.write((f"HTTP/1.1 200 OK\r\nContent-Type: {kind}\r\nContent-Length: {stat.st_size}\r\n"
                  f"Cache-Control: {cache}\r\nLast-Modified: {modified}\r\n"
                  f"Connection: {'keep-alive' if keep else 'close'}\r\n\r\n").encode())
    if method == "GET":
        with open(file, "rb") as f:
            while data := f.read(CHUNK):
                writer.write(data)
                await writer.drain()
    await writer.drain()
    return keep


async def client(reader, writer, root, relay):
    try:
        while True:
            head = await read_head(reader)
            if head is None:
                break
            method, target, headers = parse(head)
            if headers.get("upgrade", "").lower() == "websocket":
                await to_relay(head, reader, writer, relay)
                return
            if not await serve_file(method, target, headers, writer, root):
                break
    except (ConnectionError, OSError):
        pass
    finally:
        try:
            writer.close()
        except Exception:
            pass


async def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--port", type=int, default=8090)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent / "client" / "dist")
    parser.add_argument("--relay", default="127.0.0.1:2290")
    args = parser.parse_args()
    root = args.root.resolve()
    server = await asyncio.start_server(lambda r, w: client(r, w, root, args.relay), args.bind, args.port,
                                        limit=1 << 16)
    print(f"site on http://{args.bind}:{args.port} ({root}), WebSocket -> {args.relay}", flush=True)
    async with server:
        await server.serve_forever()


if __name__ == "__main__":
    asyncio.run(main())
