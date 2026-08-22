// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";

import { setControlPlaneToken, type SchemaInventoryReport } from "../api";
import { SchemaLedger } from "./SchemaLedger";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const report: SchemaInventoryReport = {
  inventory_version: 1,
  active: 1,
  reserved: 1,
  deprecated: 1,
  counted: 3,
  failed_queries: [],
  items: [
    { kind: "node", name: "Memory", lifecycle: "active", owner: "memory", milestone: null, producer: "add", consumer: "search", e2e: "read_path_e2e", migration: null, purpose: "Atomic memory.", count_key: "memory_count", count: 5359 },
    { kind: "node", name: "Session", lifecycle: "reserved", owner: "session", milestone: "v0.18", producer: null, consumer: null, e2e: null, migration: null, purpose: "Dormant session shape.", count_key: "session_count", count: 0 },
    { kind: "edge", name: "OLD_EDGE", lifecycle: "deprecated", owner: "schema", milestone: null, producer: null, consumer: null, e2e: null, migration: "backup first", purpose: "Old edge.", count_key: "old_edge_count", count: 0 },
  ],
};

afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  document.body.replaceChildren();
});

test("filters the interactive ledger by physical family and lifecycle", async () => {
  setControlPlaneToken("a".repeat(64));
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(report), { status: 200, headers: { "Content-Type": "application/json" } })));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => { root.render(<SchemaLedger />); await new Promise(resolve => setTimeout(resolve, 0)); });

  expect(host.textContent).toContain("Memory");
  expect(host.textContent).toContain("Session");
  await act(async () => button(host, "Edges").click());
  expect(host.textContent).toContain("OLD_EDGE");
  expect(host.textContent).not.toContain("Atomic memory.");
  await act(async () => button(host, "Active").click());
  expect(host.querySelectorAll(".schema-record")).toHaveLength(0);
  await act(async () => root.unmount());
});

function button(host: HTMLElement, name: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(candidate => candidate.textContent?.includes(name));
  if (!(found instanceof HTMLButtonElement)) throw new Error(`button not found: ${name}`);
  return found;
}
