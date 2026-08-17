// @vitest-environment jsdom

import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, test, vi } from "vitest";

import { Metric } from "./components";

(globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

afterEach(() => {
  document.body.replaceChildren();
});

test("renders an overview metric as an accessible drill-down action", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const activate = vi.fn();
  const root = createRoot(host);

  await act(async () => {
    root.render(
      <Metric
        action="Open live roster"
        detail="Live swarm heartbeat"
        eyebrow="Agents online"
        onActivate={activate}
        value="3 / 65"
      />,
    );
  });

  const button = host.querySelector("button");
  expect(button?.getAttribute("aria-label")).toContain("Open live roster");
  expect(button?.textContent).toContain("3 / 65");
  button?.click();
  expect(activate).toHaveBeenCalledOnce();

  await act(async () => root.unmount());
});
