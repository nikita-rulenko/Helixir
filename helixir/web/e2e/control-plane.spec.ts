import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Page, type Route } from "@playwright/test";

const overview = {
  actor_id: "operator", access_scope: "global", mode: "collective+insights",
  memories: 42, graph_nodes: 91, principals: 3, agents: 2, active_agents: 1,
  workspaces: 3, entities: 31, concepts: 7,
};
const discovery = {
  phase: "ready",
  state: {
    backend: { kind: "existing_local", host: "127.0.0.1", port: 6970, healthy: true, schema_compatible: true },
    ollama: { installed: true, running: true, models: ["nomic-embed-text"] },
    nli_installed: true, central_config_matches: true,
    client_registered: { codex: true, cursor: true },
    rbac: { enabled: true, migration_active: false, default_group_exists: true, onboarding_group_exists: true, moirai_group_exists: true, global_admins: ["operator"], registered_principals: ["operator"] },
  },
};
const plan = {
  steps: [
    { action: { kind: "verify_backend" }, required: true, reason: "verify backend" },
    { action: { kind: "run_doctor" }, required: true, reason: "prove readiness" },
  ],
};
const operation = {
  operation_id: "op-release-gate", plan_fingerprint: "fixture", status: "succeeded",
  created_at: "2026-08-17T00:00:00Z", updated_at: "2026-08-17T00:00:01Z", plan,
  events: [], report: { steps: plan.steps.map(step => ({ action: step.action, succeeded: true, detail: null })), ready: true, rollback_attempted: false, rollback_error: null },
  error: null, resumable: false,
};

async function mockAdminApi(page: Page) {
  await page.route("**/api/v1/**", async route => {
    const url = new URL(route.request().url());
    const path = url.pathname.replace("/api/v1", "");
    if (path === "/meta") return json(route, { product: "helixir", version: "0.16.0", api_version: "v1", phase: "admin", transport: "http", runtime: "control-plane-container", host_operations_available: true });
    if (path === "/overview") return json(route, overview);
    if (path === "/discovery") return json(route, discovery);
    if (path === "/install/plan") return json(route, plan);
    if (path === "/install/operations" || path.endsWith("/resume")) return json(route, operation);
    if (path.includes("/events")) return route.fulfill({ status: 200, contentType: "text/event-stream", body: "" });
    if (path === "/install/operations/op-release-gate") return json(route, operation);
    if (path === "/install/verify") return json(route, { ready: true, checks: [{ name: "RBAC", status: "pass", detail: "permanent", required: true }] });
    return json(route, {});
  });
}

test("first-run plan, apply and verify journey is complete", async ({ page }) => {
  await mockAdminApi(page);
  await page.goto("/#token=" + "a".repeat(64));
  await page.getByRole("button", { name: /Installation 02/ }).click();
  await expect(page.getByText("existing local", { exact: false }).first()).toBeVisible();
  await page.getByRole("button", { name: /Review exact plan/ }).click();
  await expect(page.getByRole("button", { name: /Apply exact plan/ })).toBeVisible();
  await page.getByRole("button", { name: /Apply exact plan/ }).click();
  await expect(page.getByRole("button", { name: /Verify installation/ })).toBeVisible();
  await page.getByRole("button", { name: /Verify installation/ }).click();
  await expect(page.getByText("Ready for agents")).toBeVisible();
});

test("missing and non-admin credentials fail closed", async ({ page }) => {
  await page.route("**/api/v1/**", route => route.fulfill({ status: 403, contentType: "application/json", body: JSON.stringify({ message: "Global admin access required" }) }));
  await page.goto("/");
  await expect(page.getByText("Administrators", { exact: false })).toBeVisible();
  await expect(page.getByText("No graph data was exposed")).toHaveCount(0);
});

test("expired browser credential exposes only the reconnect boundary", async ({ page }) => {
  await page.route("**/api/v1/**", route => route.fulfill({ status: 401, contentType: "application/json", body: JSON.stringify({ message: "Admin session token is missing or no longer valid" }) }));
  await page.goto("/");
  await expect(page.getByRole("heading", { name: /Reconnect to/ })).toBeVisible();
  await expect(page.getByLabel("Browser token")).toBeVisible();
  await expect(page.getByText("No graph data was exposed")).toBeVisible();
});

test("overview is accessible and usable at desktop and mobile widths", async ({ page }, testInfo) => {
  await mockAdminApi(page);
  await page.goto("/#token=" + "b".repeat(64));
  await expect(page.getByText("5", { exact: true })).toHaveCount(0);
  await expect(page.getByText("42", { exact: true }).first()).toBeVisible();
  const results = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa"]).analyze();
  expect(results.violations.filter(item => ["critical", "serious"].includes(item.impact ?? ""))).toEqual([]);
  if (testInfo.project.name === "mobile") {
    await expect(page.getByRole("button", { name: /Memory field 04/ })).toBeVisible();
  }
});

test("polling soak keeps request fan-out and DOM bounded", async ({ page }) => {
  let overviewRequests = 0;
  await mockAdminApi(page);
  page.on("request", request => { if (request.url().includes("/api/v1/overview")) overviewRequests += 1; });
  await page.goto("/#token=" + "c".repeat(64));
  const refresh = page.getByRole("button", { name: /Refresh live control-plane data/ });
  for (let index = 0; index < 20; index += 1) {
    await refresh.click();
    await expect(refresh).toBeEnabled();
  }
  expect(overviewRequests).toBeLessThanOrEqual(32);
  expect(await page.locator("*").count()).toBeLessThan(650);
});

function json(route: Route, body: unknown) {
  return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(body) });
}
