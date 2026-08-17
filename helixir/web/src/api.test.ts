// @vitest-environment jsdom

import { beforeEach, expect, test, vi } from "vitest";

import {
  apiEventStream,
  apiGet,
  sessionExpiredEvent,
  setControlPlaneToken,
} from "./api";

const stableToken = "a".repeat(64);

beforeEach(() => {
  sessionStorage.clear();
  history.replaceState(null, "", "/");
  vi.restoreAllMocks();
});

test("replays authenticated SSE after the supplied cursor", async () => {
  setControlPlaneToken(stableToken);
  const seen: number[] = [];
  let requested = "";
  let lastEventId = "";
  const payload = {
    operation_id: "op_test",
    sequence: 8,
    event_id: "op_test:8",
    step_id: "step-0002",
    at: "2026-08-16T10:00:00Z",
    kind: "progress",
    install: null,
    detail: null,
  };
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    requested = String(input);
    lastEventId = new Headers(init?.headers).get("Last-Event-ID") ?? "";
    return new Response(`id: op_test:8\nevent: operation\ndata: ${JSON.stringify(payload)}\n\n`, {
      status: 200,
      headers: { "Content-Type": "text/event-stream" },
    });
  }));

  const cursor = await apiEventStream(
    "/install/operations/op_test/events",
    7,
    event => seen.push(event.sequence),
    new AbortController().signal,
  );

  expect(requested).toContain("after=7");
  expect(lastEventId).toBe("7");
  expect(seen).toEqual([8]);
  expect(cursor).toBe(8);
});

test("keeps the stable bearer token for requests after a server restart", async () => {
  const authorizations: string[] = [];
  vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    authorizations.push(new Headers(init?.headers).get("Authorization") ?? "");
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }));

  setControlPlaneToken(stableToken);
  await apiGet<{ ok: boolean }>("/overview");
  // A process restart does not clear the tab's sessionStorage.
  await apiGet<{ ok: boolean }>("/overview");

  expect(authorizations).toEqual([
    `Bearer ${stableToken}`,
    `Bearer ${stableToken}`,
  ]);
});

test("captures an authenticated URL once and removes the token from the address bar", async () => {
  history.replaceState(null, "", `/#token=${stableToken}`);
  let authorization = "";
  vi.stubGlobal("fetch", vi.fn(async (_input: RequestInfo | URL, init?: RequestInit) => {
    authorization = new Headers(init?.headers).get("Authorization") ?? "";
    return new Response(JSON.stringify({ ok: true }), {
      status: 200,
      headers: { "Content-Type": "application/json" },
    });
  }));

  await apiGet<{ ok: boolean }>("/meta");

  expect(authorization).toBe(`Bearer ${stableToken}`);
  expect(location.hash).toBe("");
  expect(location.pathname).toBe("/");
});

test("turns HTTP 401 into a global session-recovery signal", async () => {
  setControlPlaneToken("b".repeat(64));
  const expired = vi.fn();
  window.addEventListener(sessionExpiredEvent, expired, { once: true });
  vi.stubGlobal("fetch", vi.fn(async () => new Response("", { status: 401 })));

  await expect(apiGet("/overview")).rejects.toMatchObject({ status: 401 });

  expect(expired).toHaveBeenCalledOnce();
});
