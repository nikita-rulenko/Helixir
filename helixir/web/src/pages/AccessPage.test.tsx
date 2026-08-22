// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";

import { setControlPlaneToken, type AccessProjection } from "../api";
import { AccessPage } from "./AccessPage";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const instances = [
  { agent_id: "codex", principal_id: "codex", name: "Codex", role: "coordinator", host: "studio", status: "idle", last_seen: "2026-08-22T08:00:00Z", age_seconds: 7200, active: false },
  { agent_id: "codex/release-gate", principal_id: "codex", name: "Release gate", role: "tester", host: "runner-01", status: "working", last_seen: "2026-08-22T10:00:00Z", age_seconds: 8, active: true },
  { agent_id: "codex/client-smoke", principal_id: "codex", name: "Client smoke", role: "tester", host: "runner-02", status: "working", last_seen: "2026-08-22T10:00:00Z", age_seconds: 11, active: true },
  { agent_id: "codex/ui-audit", principal_id: "codex", name: "UI audit", role: "designer", host: "studio", status: "working", last_seen: "2026-08-22T09:59:00Z", age_seconds: 18, active: true },
];

const access: AccessProjection = {
  active_window_secs: 600,
  agents: instances,
  agent_families: [
    { principal_id: "codex", active: true, instance_count: 4, active_instances: 3, hosts: ["runner-01", "runner-02", "studio"], instances },
  ],
  subagents: instances.filter(instance => instance.agent_id !== instance.principal_id),
  principals: [{ subject_id: "codex", global_roles: ["admin"], groups: [] }],
  groups: [],
  dedup_groups: [],
  contributors: [],
  contributor_sample_size: 0,
};

afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  document.body.replaceChildren();
});

test("keeps one logical Codex online through three child leases while its root is stale", async () => {
  setControlPlaneToken("a".repeat(64));
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify(access), { status: 200, headers: { "Content-Type": "application/json" } })));
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);

  await act(async () => { root.render(<AccessPage initialTab="agents" />); await new Promise(resolve => setTimeout(resolve, 0)); });

  expect(host.textContent).toContain("1 agents online · 3 subagents online");
  expect(host.textContent).toContain("Presence comes from Helixir activity");
  expect(host.querySelectorAll(".agent-family-accordion > details")).toHaveLength(1);
  expect(host.textContent).toContain("3 subagents");
  expect(host.textContent).toContain("codex/release-gate");
  expect(host.querySelectorAll(".instance-kind.is-root")).toHaveLength(1);
  expect(host.querySelectorAll(".instance-kind:not(.is-root)")).toHaveLength(3);
  expect([...host.querySelectorAll("button")].filter(button => button.textContent?.includes("Prune instance"))).toHaveLength(0);

  await act(async () => button(host, "Online only").click());
  expect(host.querySelectorAll(".agent-family-accordion > details")).toHaveLength(1);

  await act(async () => root.unmount());
});

function button(host: HTMLElement, name: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(candidate => candidate.textContent?.includes(name));
  if (!(found instanceof HTMLButtonElement)) throw new Error(`button not found: ${name}`);
  return found;
}
