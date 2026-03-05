#!/usr/bin/env python3
"""
Article text extractor using newspaper4k.
Runs as an asyncio Unix socket server for IPC with Go.
"""

import asyncio
import argparse
import json
import signal
import struct
import sys
import warnings
from pathlib import Path

from newspaper import Article

# Suppress asyncio warnings on forced shutdown
warnings.filterwarnings("ignore", category=RuntimeWarning, message=".*coroutine.*")

# Protocol: 4-byte length prefix (big-endian uint32) + JSON payload
HEADER_SIZE = 4


async def extract_text(url: str, html: str) -> dict:
    """Extract article text from HTML using newspaper4k."""
    try:
        article = Article(url)
        article.download(input_html=html)
        article.parse()

        text = article.text.strip()
        if not text:
            return {"text": "", "status": "failed", "error": "empty_text"}

        return {"text": text, "status": "ok"}
    except Exception as e:
        return {"text": "", "status": "failed", "error": str(e)}


async def read_message(reader: asyncio.StreamReader) -> dict | None:
    """Read a length-prefixed JSON message."""
    header = await reader.readexactly(HEADER_SIZE)
    if not header:
        return None

    length = struct.unpack(">I", header)[0]
    data = await reader.readexactly(length)
    return json.loads(data.decode("utf-8"))


async def write_message(writer: asyncio.StreamWriter, msg: dict) -> None:
    """Write a length-prefixed JSON message."""
    data = json.dumps(msg).encode("utf-8")
    header = struct.pack(">I", len(data))
    writer.write(header + data)
    await writer.drain()


async def handle_client(reader: asyncio.StreamReader, writer: asyncio.StreamWriter):
    """Handle a single client connection."""
    try:
        while True:
            try:
                msg = await read_message(reader)
            except asyncio.IncompleteReadError:
                break
            except Exception:
                break

            if msg is None:
                break

            # Check for shutdown command
            if msg.get("command") == "shutdown":
                await write_message(writer, {"status": "ok", "message": "shutting_down"})
                break

            # Extract text
            url = msg.get("url", "")
            html = msg.get("html", "")
            result = await extract_text(url, html)
            await write_message(writer, result)

    except Exception:
        pass  # Silently handle errors on shutdown
    finally:
        try:
            writer.close()
            await writer.wait_closed()
        except Exception:
            pass


async def run_server(socket_path: str):
    """Run the Unix socket server."""
    # Remove existing socket file if present
    socket_file = Path(socket_path)
    if socket_file.exists():
        socket_file.unlink()

    server = await asyncio.start_unix_server(handle_client, path=socket_path)

    # Signal readiness to parent process
    print(f"READY:{socket_path}", flush=True)

    async with server:
        try:
            await server.serve_forever()
        except asyncio.CancelledError:
            pass
        except Exception:
            pass

    # Cleanup socket file
    if socket_file.exists():
        socket_file.unlink()


def main():
    # Suppress task destroyed warnings
    import logging
    logging.getLogger("asyncio").setLevel(logging.CRITICAL)

    parser = argparse.ArgumentParser(description="Article text extractor server")
    parser.add_argument("--socket", required=True, help="Unix socket path")
    args = parser.parse_args()

    socket_file = Path(args.socket)

    def cleanup():
        if socket_file.exists():
            socket_file.unlink()

    # Handle signals for graceful shutdown
    loop = asyncio.new_event_loop()
    asyncio.set_event_loop(loop)

    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(sig, loop.stop)

    try:
        loop.run_until_complete(run_server(args.socket))
    except (KeyboardInterrupt, RuntimeError):
        pass
    finally:
        # Cancel all pending tasks
        pending = asyncio.all_tasks(loop)
        for task in pending:
            task.cancel()

        # Run until all tasks are cancelled
        if pending:
            try:
                loop.run_until_complete(asyncio.gather(*pending, return_exceptions=True))
            except RuntimeError:
                # Event loop can be stopped by signal handlers during shutdown.
                pass

        loop.close()
        cleanup()


if __name__ == "__main__":
    main()
