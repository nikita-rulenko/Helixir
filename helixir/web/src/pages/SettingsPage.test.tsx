// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";

import { setControlPlaneToken, type SettingsSnapshot } from "../api";
import { SettingsPage } from "./SettingsPage";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const settings: SettingsSnapshot = {
  config_path: "~/.helixir/helixir.toml", locked_fields: [], mode: "Collective",
  database: { host: "127.0.0.1", port: 6970, instance: "helixir" },
  reasoning: { provider: "cerebras", model: "gpt-oss-120b", base_url: "https://api.cerebras.ai/v1", temperature: 0.1, api_key_configured: true },
  embeddings: { provider: "ollama", model: "nomic-embed-text", url: "http://127.0.0.1:11434", api_key_configured: false },
  swarm: { active_window_secs: 120, presence_ttl_secs: 600 },
  watchdog: { enabled: true, sample_interval_secs: 30, mem_alert_pct: 80, mem_restart_pct: 95, allow_container_restart: true, allow_cache_reclaim: true, backup_interval_hours: 6, backup_keep: 7 },
};

afterEach(() => { vi.restoreAllMocks(); sessionStorage.clear(); document.body.replaceChildren(); });

test("reviews an allowlisted diff before applying settings", async () => {
  setControlPlaneToken("a".repeat(64));
  let posted: unknown = null;
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const path = String(input);
    if (path.endsWith("/settings") && init?.method === "POST") {
      posted = JSON.parse(String(init.body));
      return json({ apply: { changed: true, config_backup: "~/.helixir/helixir.toml.bak", reload_required: true, settings: { ...settings, mode: "Insights" } }, reload: { signalled_processes: 1, failed_signals: 0, restart_required: [] } });
    }
    if (path.endsWith("/settings")) return json(settings);
    if (path.endsWith("/backups")) return json({ available: true, reason: null, directory: "~/.helixir/backups", retention: 7, archives: [] });
    throw new Error(`unexpected request: ${path}`);
  }));
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  await act(async () => { root.render(<SettingsPage hostOperationsAvailable />); await Promise.resolve(); });
  await act(async () => { await new Promise(resolve => setTimeout(resolve, 0)); });

  const mode = [...host.querySelectorAll("select")].find(select => select.value === "Collective")!;
  await act(async () => { mode.value = "Insights"; mode.dispatchEvent(new Event("change", { bubbles: true })); });
  await act(async () => button(host, "Review exact change").click());
  expect(host.textContent).toContain("Review before writing");
  expect(host.textContent).toContain("Memory mode");
  await act(async () => { button(host, "Apply reviewed changes").click(); await new Promise(resolve => setTimeout(resolve, 0)); });
  expect(posted).toEqual({ mode: "Insights" });
  expect(host.textContent).toContain("Configuration saved with an automatic rollback copy");
  await act(async () => root.unmount());
});

test("requires the exact restore phrase", async () => {
  setControlPlaneToken("b".repeat(64));
  vi.stubGlobal("fetch", vi.fn(async (input: RequestInfo | URL) => String(input).endsWith("/settings") ? json(settings) : json({ available: true, reason: null, directory: "~/.helixir/backups", retention: 7, archives: [{ id: "helixdb-manual-20260819.tar.gz", created_at: "2026-08-19T09:00:00Z", size_bytes: 1048576, kind: "manual" }] })));
  const host = document.createElement("div"); document.body.append(host); const root = createRoot(host);
  await act(async () => { root.render(<SettingsPage hostOperationsAvailable />); await new Promise(resolve => setTimeout(resolve, 0)); });
  await act(async () => button(host, "helixdb-manual-20260819.tar.gz").click());
  expect(button(host, "Restore with safety copy").disabled).toBe(true);
  await act(async () => root.unmount());
});

function json(value: unknown): Response { return new Response(JSON.stringify(value), { status: 200, headers: { "Content-Type": "application/json" } }); }
function button(host: HTMLElement, name: string): HTMLButtonElement {
  const found = [...host.querySelectorAll("button")].find(candidate => candidate.textContent?.includes(name));
  if (!(found instanceof HTMLButtonElement)) throw new Error(`button not found: ${name}`);
  return found;
}
