// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";

import App from "./App";
import { setControlPlaneToken } from "./api";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  vi.restoreAllMocks();
  sessionStorage.clear();
  window.location.hash = "";
  document.body.replaceChildren();
});

test("waits for discovery before choosing installation defaults", async () => {
  window.location.hash = "#setup";
  setControlPlaneToken("release-test-token");

  let resolveDiscovery!: (response: Response) => void;
  let resolveOverview!: (response: Response) => void;
  const discovery = new Promise<Response>(resolve => { resolveDiscovery = resolve; });
  const overview = new Promise<Response>(resolve => { resolveOverview = resolve; });
  vi.stubGlobal("fetch", vi.fn((input: RequestInfo | URL) => {
    const path = String(input);
    if (path.endsWith("/meta")) {
      return Promise.resolve(jsonResponse({
        product: "helixir",
        version: "0.16.1",
        api_version: "v1",
        phase: "admin",
        transport: "http",
        runtime: "control-plane-container",
        host_operations_available: true,
      }));
    }
    if (path.endsWith("/discovery")) return discovery;
    if (path.endsWith("/overview")) return overview;
    throw new Error(`unexpected request: ${path}`);
  }));

  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<App />);
    await Promise.resolve();
  });
  expect(host.textContent).toContain("Reading the host before choosing safe installation defaults.");

  await act(async () => {
    resolveDiscovery(jsonResponse({
      phase: "ready",
      state: {
        backend: {
          kind: "existing_local",
          host: "127.0.0.1",
          port: 6970,
          healthy: true,
          schema_compatible: true,
        },
        ollama: { installed: true, running: true, models: ["nomic-embed-text"] },
        nli_installed: true,
        central_config_matches: true,
        client_registered: { codex: true },
        rbac: {
          enabled: true,
          migration_active: false,
          default_group_exists: true,
          onboarding_group_exists: true,
          moirai_group_exists: true,
          global_admins: ["codex"],
          registered_principals: ["codex"],
        },
      },
    }));
    resolveOverview(jsonResponse({
      actor_id: "codex",
      access_scope: "global",
      mode: "collective+insights",
      memories: 1,
      graph_nodes: 1,
      principals: 1,
      agents: 1,
      active_agents: 1,
      workspaces: 3,
      entities: 1,
      concepts: 1,
    }));
    await Promise.all([discovery, overview]);
  });

  expect(host.textContent).toContain("existing local");
  expect(buttonNamed(host, "Keep this database").getAttribute("aria-pressed")).toBe("true");
  expect(buttonNamed(host, "Insights").getAttribute("aria-pressed")).toBe("true");

  await act(async () => root.unmount());
});

function jsonResponse(value: unknown): Response {
  return new Response(JSON.stringify(value), {
    status: 200,
    headers: { "Content-Type": "application/json" },
  });
}

function buttonNamed(host: HTMLElement, name: string): HTMLButtonElement {
  const button = [...host.querySelectorAll("button")].find(candidate => candidate.textContent?.includes(name));
  if (!(button instanceof HTMLButtonElement)) throw new Error(`button not found: ${name}`);
  return button;
}
