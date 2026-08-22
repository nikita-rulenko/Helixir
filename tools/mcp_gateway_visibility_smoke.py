#!/usr/bin/env python3
"""Prove that a remote host can use Helixir only through its MCP gateway."""

from __future__ import annotations

import argparse
import json
import sys
import time
import urllib.error
import urllib.request


class McpSession:
    def __init__(self, endpoint: str) -> None:
        self.endpoint = endpoint.rstrip("/")
        self.session_id: str | None = None
        self.next_id = 1

    def initialize(self) -> dict:
        result, headers = self._post(
            {
                "jsonrpc": "2.0",
                "id": self.next_id,
                "method": "initialize",
                "params": {
                    "protocolVersion": "2025-06-18",
                    "capabilities": {},
                    "clientInfo": {
                        "name": "helixir-release-gate",
                        "version": "1",
                    },
                },
            }
        )
        self.next_id += 1
        self.session_id = headers.get("mcp-session-id")
        if not self.session_id:
            raise RuntimeError("initialize response omitted mcp-session-id")
        self._post({"jsonrpc": "2.0", "method": "notifications/initialized"})
        return result

    def request(self, method: str, params: dict) -> dict:
        result, _ = self._post(
            {
                "jsonrpc": "2.0",
                "id": self.next_id,
                "method": method,
                "params": params,
            }
        )
        self.next_id += 1
        return result

    def call_tool(self, name: str, arguments: dict) -> object:
        result = self.request(
            "tools/call", {"name": name, "arguments": arguments}
        )
        if result.get("isError"):
            raise RuntimeError(f"{name} returned isError")
        for item in result.get("content", []):
            text = item.get("text")
            if text is not None:
                return json.loads(text)
        raise RuntimeError(f"{name} returned no JSON text content")

    def _post(self, payload: dict) -> tuple[dict, dict[str, str]]:
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
        try:
            with urllib.request.urlopen(request, timeout=20) as response:
                body = response.read().decode()
                response_headers = {
                    key.lower(): value for key, value in response.headers.items()
                }
        except urllib.error.HTTPError as error:
            detail = error.read().decode(errors="replace")[:300]
            raise RuntimeError(f"HTTP {error.code}: {detail}") from error
        if not body.strip():
            return {}, response_headers
        content_type = response_headers.get("content-type", "")
        if "text/event-stream" in content_type:
            data = next(
                (line[6:] for line in body.splitlines() if line.startswith("data: ")),
                None,
            )
            if data is None:
                raise RuntimeError("empty MCP SSE response")
            envelope = json.loads(data)
        else:
            envelope = json.loads(body)
        if "error" in envelope:
            raise RuntimeError(f"MCP error: {envelope['error']}")
        return envelope.get("result", {}), response_headers


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--gateway", required=True)
    parser.add_argument("--database", required=True)
    parser.add_argument("--principal", required=True)
    parser.add_argument("--owner", required=True)
    parser.add_argument("--forbidden-owner")
    parser.add_argument("--tag", required=True)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    gateway = McpSession(args.gateway)
    initialized = gateway.initialize()
    server = initialized.get("serverInfo", {})
    tools = gateway.request("tools/list", {}).get("tools", [])
    tool_names = {tool.get("name") for tool in tools}
    required = {
        "enroll_client",
        "search_memory",
        "agent_heartbeat",
        "think_start",
        "think_conclude",
        "think_commit",
        "agent_farewell",
        "swarm_status",
    }
    missing = sorted(required - tool_names)
    if missing:
        raise RuntimeError(f"gateway is missing tools: {', '.join(missing)}")

    enrollment = gateway.call_tool(
        "enroll_client", {"actor_id": args.principal}
    )
    if enrollment.get("principal_id") != args.principal:
        raise RuntimeError("gateway enrolled a different principal")
    if enrollment.get("group_id") != "onboarding":
        raise RuntimeError("new client was not bounded to onboarding")

    instance_id = f"{args.principal}-visibility-smoke"
    try:
        heartbeat = gateway.call_tool(
            "agent_heartbeat",
            {
                "actor_id": args.principal,
                "agent_id": instance_id,
                "status": "remote-smoke",
            },
        )
        if heartbeat.get("available") is not True:
            raise RuntimeError(
                "gateway swarm is unavailable; release gate requires "
                "HELIXIR_MODE=collective"
            )
        if heartbeat.get("principal_id") != args.principal:
            raise RuntimeError("heartbeat was attached to a different principal")
        swarm = gateway.call_tool(
            "swarm_status", {"actor_id": args.principal}
        )
        families = swarm.get("families", [])
        if not any(
            family.get("principal_id") == args.principal for family in families
        ):
            raise RuntimeError("swarm status omitted the enrolled principal family")

        session_id = f"{args.principal}-{args.tag}"
        start = gateway.call_tool(
            "think_start",
            {
                "actor_id": args.principal,
                "session_id": session_id,
                "initial_thought": f"verify remote gateway {args.tag}",
            },
        )
        root_idx = start.get("root_thought_idx", 0)
        gateway.call_tool(
            "think_conclude",
            {
                "actor_id": args.principal,
                "session_id": session_id,
                "conclusion": f"Remote MCP visibility gate {args.tag} passed",
                "supporting_idx": [root_idx],
            },
        )
        committed = gateway.call_tool(
            "think_commit",
            {
                "actor_id": args.principal,
                "session_id": session_id,
                "user_id": args.owner,
                "group_id": "onboarding",
            },
        )
        if not committed.get("memory_id"):
            raise RuntimeError("think_commit returned no memory_id")

        found = False
        for _ in range(30):
            rows = gateway.call_tool(
                "search_memory",
                {
                    "actor_id": args.principal,
                    "user_id": args.owner,
                    "query": args.tag,
                    "mode": "full",
                },
            )
            if any(args.tag in row.get("content", "") for row in rows):
                found = True
                break
            time.sleep(1)
        if not found:
            raise RuntimeError("committed memory was not readable through the gateway")

        visibility_enforced = None
        if args.forbidden_owner:
            try:
                forbidden_rows = gateway.call_tool(
                    "search_memory",
                    {
                        "actor_id": args.principal,
                        "user_id": args.forbidden_owner,
                        "query": "memory",
                        "mode": "full",
                    },
                )
            except Exception:
                visibility_enforced = True
            else:
                visibility_enforced = not forbidden_rows
            if not visibility_enforced:
                raise RuntimeError("onboarding client read a forbidden owner's memory")

        try:
            McpSession(args.database).initialize()
        except Exception:
            database_rejected = True
        else:
            database_rejected = False
        if not database_rejected:
            raise RuntimeError("HelixDB port was accepted as an MCP gateway")
    finally:
        primary_error = sys.exception()
        farewell_error = None
        for agent_id in (instance_id, args.principal):
            try:
                gateway.call_tool(
                    "agent_farewell",
                    {"actor_id": args.principal, "agent_id": agent_id},
                )
            except Exception as error:
                farewell_error = error
        if primary_error is None and farewell_error is not None:
            raise farewell_error

    print(
        json.dumps(
            {
                "ok": True,
                "server": server.get("name"),
                "principal": args.principal,
                "group": enrollment.get("group_id"),
                "memory_id": committed.get("memory_id"),
                "helixdb_port_rejected": database_rejected,
                "visibility_enforced": visibility_enforced,
            },
            sort_keys=True,
        )
    )


if __name__ == "__main__":
    main()
