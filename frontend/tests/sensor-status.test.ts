import assert from "node:assert/strict";
import { resolve } from "node:path";
import { test } from "node:test";
import { createElement, type ComponentType } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import type { ObservationType, UserAction } from "../src/types";

test("sensor grid renders all backend sensor types and falls back for newer types", async (test) => {
  const vite = await createServer({
    root: resolve(import.meta.dirname, ".."), logLevel: "silent",
    optimizeDeps: { noDiscovery: true },
    server: { middlewareMode: true, hmr: false, watch: null },
  });
  test.after(() => vite.close());
  const { SensorStatusGrid } = await vite.ssrLoadModule("/src/components/SensorStatusGrid.tsx") as {
    SensorStatusGrid: ComponentType<{ actions: UserAction[] }>;
  };
  const types: [ObservationType, string][] = [
    ["Idle", "Idle detection"], ["FocusedApplication", "Focused application"],
    ["Keyboard", "Keyboard"], ["MouseClick", "Mouse clicks"], ["MouseMove", "Mouse movement"],
    ["BrowserActivity", "Browser activity"], ["MouseKeyboardActivity", "Mouse and keyboard activity"],
  ];
  const actions: UserAction[] = types.map(([observation_type], index) => ({
    id: String(index), device_id: "device", occurred_at: "2026-11-28T10:00:00Z",
    kind: { AdapterStatus: { adapter_id: `adapter-${index}`, observation_type, status: "Available" } },
  }));
  actions.push({
    id: "future", device_id: "device", occurred_at: "2026-11-28T10:00:00Z",
    kind: { AdapterStatus: { adapter_id: "new-adapter", observation_type: "FutureSensor", status: { Unavailable: { reason: "Waiting for input" } } } },
  } as unknown as UserAction);
  const html = renderToStaticMarkup(createElement(SensorStatusGrid, { actions }));
  for (const [, label] of types) assert(html.includes(`aria-label="${label}: online"`), label);
  assert(html.includes('aria-label="FutureSensor: offline"'));
  assert(html.includes("Waiting for input"));
});
