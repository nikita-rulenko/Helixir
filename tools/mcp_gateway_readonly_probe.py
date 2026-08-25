#!/usr/bin/env python3
"""Probe a managed gateway without mutating durable memory."""

from __future__ import annotations

import argparse
import json
import urllib.request


class Session:
    def __init__(self, endpoint: str) -> None:
        self.endpoint = endpoint
        self.session_id: str | None = None
        self.request_id = 1

    def post(self, payload: dict) -> dict:
        headers = {
            "Accept": "application/json, text/event-stream",
            "Content-Type": "application/json",
        }
        if self.session_id:
            headers["mcp-session-id"] = self.session_id
        request = urllib.request.Request(
            self.endpoint,
            data=json.dumps(payload).encode(),
            headers=headers,
            method="POST",
        )
        with urllib.request.urlopen(request, timeout=20) as response:
            body = response.read().decode()
            response_headers = {
                key.lower(): value for key, value in response.headers.items()
            }
        if not body.strip():
            return {}
        if "text/event-stream" in response_headers.get("content-type", ""):
            event = next(
                line[6:] for line in body.splitlines() if line.startswith("data: ")
            )
            envelope = json.loads(event)
        else:
            envelope = json.loads(body)
        if "error" in envelope:
            raise RuntimeError(envelope["error"])
        return envelope.get("result", {})

    def request(self, method: str, params: dict) -> dict:
        result = self.post(
            {
                "jsonrpc": "2.0",
                "id": self.request_id,
                "method": method,
                "params": params,
            }
        )
        self.request_id += 1
        return result

def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gateway", required=True)
    parser.add_argument("--actor", required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    session = Session(args.gateway)

    headers = {
        "Accept": "application/json, text/event-stream",
        "Content-Type": "application/json",
    }
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2025-06-18",
            "capabilities": {},
            "clientInfo": {
                "name": "helixir-systemd-release-gate",
                "version": "1",
            },
        },
    }
    request = urllib.request.Request(
        args.gateway,
        data=json.dumps(payload).encode(),
        headers=headers,
        method="POST",
    )
    with urllib.request.urlopen(request, timeout=20) as response:
        body = response.read().decode()
        session.session_id = response.headers.get("mcp-session-id")
        content_type = response.headers.get("content-type", "")
    if not session.session_id:
        raise RuntimeError("initialize omitted mcp-session-id")
    session.request_id = 2
    if "text/event-stream" in content_type:
        event = next(
            line[6:] for line in body.splitlines() if line.startswith("data: ")
        )
        initialized = json.loads(event).get("result", {})
    else:
        initialized = json.loads(body).get("result", {})
    session.post({"jsonrpc": "2.0", "method": "notifications/initialized"})

    tools = session.request("tools/list", {}).get("tools", [])
    names = {tool.get("name") for tool in tools}
    required = {"agent_heartbeat", "agent_farewell", "search_memory", "swarm_status"}
    missing = sorted(required - names)
    if missing:
        raise RuntimeError(f"gateway omitted required tools: {', '.join(missing)}")

    outcomes: dict[str, str] = {}
    for name, arguments in (
        (
            "agent_heartbeat",
            {
                "actor_id": args.actor,
                "agent_id": f"{args.actor}-systemd-release-gate",
                "status": "systemd-release-gate",
            },
        ),
        (
            "search_memory",
            {
                "actor_id": args.actor,
                "user_id": args.actor,
                "query": "gateway lifecycle release proof",
                "mode": "recent",
            },
        ),
        (
            "agent_farewell",
            {
                "actor_id": args.actor,
                "agent_id": f"{args.actor}-systemd-release-gate",
            },
        ),
    ):
        result = session.request("tools/call", {"name": name, "arguments": arguments})
        outcomes[name] = "error" if result.get("isError") else "ok"

    print(
        json.dumps(
            {
                "ok": True,
                "server": initialized.get("serverInfo", {}).get("name"),
                "tools": len(tools),
                "model_free_calls": outcomes,
                "proof_scope": (
                    "Linux archive/systemd/MCP transport against helixdb-mock; "
                    "semantic RBAC read/write is proved by client-quality-gate and "
                    "production macOS recall is verified separately"
                ),
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
