#!/usr/bin/env python3
"""Local synthetic API/MCP benchmark. Never contacts a public API or user config."""
import argparse
import base64
import gzip
import http.client
import json
import os
import platform
import selectors
import socket
import statistics
import subprocess
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path


class Handler(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        super().setup()
        self.connection.setsockopt(socket.IPPROTO_TCP, socket.TCP_NODELAY, 1)

    def do_POST(self):
        data = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        content = json.dumps({"results": [{"title": data["query"], "url": "https://example.com", "summary": "Synthetic result"}]}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(content)))
        self.end_headers()
        self.wfile.write(content)

    def log_message(self, *_args):
        pass


class Process:
    def __init__(self, binary, tools):
        profile = json.dumps({"version": 1, "tools": tools}, separators=(",", ":")).encode()
        env = dict(os.environ)
        env["CC_SWITCH_MCP_API_CONFIG"] = base64.b64encode(gzip.compress(profile)).decode()
        env["CC_SWITCH_MCP_API_HEADERS"] = "{}"
        self.child = subprocess.Popen([str(binary), "--mcp-api"], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=env)
        self.selector = selectors.DefaultSelector()
        self.selector.register(self.child.stdout, selectors.EVENT_READ)
        self.sequence = 0
        self.request("initialize", {"protocolVersion": "2025-11-25", "capabilities": {}, "clientInfo": {"name": "local-benchmark", "version": "1"}})
        self.send({"jsonrpc": "2.0", "method": "notifications/initialized"})

    def send(self, value):
        self.child.stdin.write(json.dumps(value).encode() + b"\n")
        self.child.stdin.flush()

    def request(self, method, params=None):
        self.sequence += 1
        self.send({"jsonrpc": "2.0", "id": self.sequence, "method": method, "params": params or {}})
        if not self.selector.select(5):
            raise RuntimeError("MCP response timed out")
        line = self.child.stdout.readline()
        if not line:
            raise RuntimeError("MCP subprocess exited: " + self.child.stderr.read().decode())
        value = json.loads(line)
        assert value["id"] == self.sequence and "error" not in value, value
        return value["result"]

    def close(self):
        self.selector.close()
        self.child.stdin.close()
        try:
            self.child.wait(3)
        except subprocess.TimeoutExpired:
            self.child.kill()
            self.child.wait()
        self.child.stdout.close()
        self.child.stderr.close()


def stats(values):
    ordered = sorted(values)
    return {"median_ms": round(statistics.median(values), 3), "p95_ms": round(ordered[int((len(ordered) - 1) * .95)], 3), "samples": len(values)}


def measure(function, count):
    values = []
    for _ in range(count):
        start = time.perf_counter()
        function()
        values.append((time.perf_counter() - start) * 1000)
    return stats(values)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, default=Path("src-tauri/target/debug/cc-switch"))
    parser.add_argument("--samples", type=int, default=200)
    args = parser.parse_args()
    if args.samples < 20:
        parser.error("--samples must be at least 20")
    binary = args.binary.resolve(strict=True)
    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    port = server.server_address[1]
    tool = {"name": "search", "description": "Synthetic search API", "method": "POST", "url": f"http://127.0.0.1:{port}/search", "inputSchema": {"type": "object", "properties": {"body": {"type": "object", "properties": {"query": {"type": "string"}}, "required": ["query"]}}, "required": ["body"], "additionalProperties": False}}
    result = {"environment": platform.platform(), "binary": str(args.binary), "response": "one synthetic search result; loopback HTTP/1.1; persistent connections", "warning": "Direct HTTP uses Python http.client; MCP uses Rust reqwest. The difference is not an isolated serialization cost."}
    try:
        cold = []
        for _ in range(10):
            start = time.perf_counter()
            process = Process(binary, [tool])
            cold.append((time.perf_counter() - start) * 1000)
            process.close()
        result["process_start_and_initialize"] = stats(cold)
        connection = http.client.HTTPConnection("127.0.0.1", port, timeout=5)

        def direct():
            connection.request("POST", "/search", json.dumps({"query": "synthetic"}), {"Content-Type": "application/json"})
            response = connection.getresponse()
            assert response.status == 200
            json.loads(response.read())

        direct()
        result["direct_http"] = measure(direct, args.samples)
        connection.close()
        process = Process(binary, [tool])
        try:
            def call():
                response = process.request("tools/call", {"name": "search", "arguments": {"body": {"query": "synthetic"}}})
                assert response["isError"] is False
            call()
            result["stdio_to_http"] = measure(call, args.samples)
            try:
                result["process_rss_kib"] = int(subprocess.check_output(["ps", "-o", "rss=", "-p", str(process.child.pid)], text=True).strip())
            except (OSError, ValueError, subprocess.CalledProcessError):
                result["process_rss_kib"] = None
        finally:
            process.close()
        tools = [dict(tool, name=f"search{i}") for i in range(100)]
        process = Process(binary, tools)
        try:
            listing = process.request("tools/list")
            result["100_tools_list_bytes"] = len(json.dumps(listing, separators=(",", ":")).encode())
            result["100_tools_list"] = measure(lambda: process.request("tools/list"), 20)
        finally:
            process.close()
        print(json.dumps(result, ensure_ascii=False, indent=2))
    finally:
        server.shutdown()
        server.server_close()


if __name__ == "__main__":
    main()
