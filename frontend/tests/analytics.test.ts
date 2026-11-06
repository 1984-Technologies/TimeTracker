import assert from "node:assert/strict";
import test from "node:test";
import {
  actionKindName,
  buildAppActivityChart,
  deriveSensorStatuses,
  sortActionsNewestFirst,
} from "../src/analytics";
import type { UserAction, UserActionKind } from "../src/types";

const deviceId = "00000000-0000-4000-8000-000000000001";

function action(id: number, occurredAt: string, kind: UserActionKind): UserAction {
  return {
    id: `00000000-0000-4000-8000-${id.toString().padStart(12, "0")}`,
    device_id: deviceId,
    occurred_at: occurredAt,
    kind,
  };
}

test("actions are sorted newest first independently of API order", () => {
  const actions = [
    action(1, "2026-01-01T10:00:00.000Z", "CollectorStarted"),
    action(2, "2026-01-01T10:15:00.000Z", "Heartbeat"),
    action(3, "2026-01-01T10:05:00.000Z", { IdleChanged: { idle: true } }),
  ];

  assert.deepEqual(
    sortActionsNewestFirst(actions).map((item) => item.id),
    [actions[1].id, actions[2].id, actions[0].id],
  );
  assert.equal(actionKindName(actions[2].kind), "IdleChanged");
});

test("the latest adapter event determines each sensor status", () => {
  const statuses = deriveSensorStatuses([
    action(1, "2026-01-01T10:00:00.000Z", {
      AdapterStatus: {
        adapter_id: "keyboard",
        observation_type: "Keyboard",
        status: { Unavailable: { reason: "Starting" } },
      },
    }),
    action(2, "2026-01-01T10:10:00.000Z", {
      AdapterStatus: {
        adapter_id: "keyboard",
        observation_type: "Keyboard",
        status: "Available",
      },
    }),
    action(3, "2026-01-01T10:05:00.000Z", {
      AdapterStatus: {
        adapter_id: "mouse",
        observation_type: "MouseMove",
        status: { Unavailable: { reason: "Permission missing" } },
      },
    }),
  ]);

  assert.equal(statuses.length, 2);
  assert.equal(statuses.find((status) => status.observationType === "Keyboard")?.available, true);
  assert.deepEqual(
    statuses.find((status) => status.observationType === "MouseMove"),
    {
      observationType: "MouseMove",
      adapterId: "mouse",
      available: false,
      reason: "Permission missing",
      occurredAt: "2026-01-01T10:05:00.000Z",
      deviceId,
    },
  );
});

test("application activity is split into normalized 15-minute buckets and excludes idle time", () => {
  const chart = buildAppActivityChart(
    [
      action(1, "2026-01-01T10:00:00.000Z", {
        FocusedApplicationChanged: { app_id: "code", title: null },
      }),
      action(2, "2026-01-01T10:05:00.000Z", {
        FocusedApplicationChanged: { app_id: "browser", title: null },
      }),
      action(3, "2026-01-01T10:12:00.000Z", { IdleChanged: { idle: true } }),
      action(4, "2026-01-01T10:18:00.000Z", { IdleChanged: { idle: false } }),
      action(5, "2026-01-01T10:20:00.000Z", {
        FocusedApplicationChanged: { app_id: "code", title: null },
      }),
      action(6, "2026-01-01T10:25:00.000Z", "CollectorStopped"),
    ],
    "2026-01-01T10:00:00.000Z",
    "2026-01-01T10:30:00.000Z",
  );

  assert.equal(chart.buckets.length, 2);
  assert.equal(chart.totalActiveMilliseconds, 19 * 60_000);

  const codeSeries = chart.series.find((series) => series.appId === "code");
  const browserSeries = chart.series.find((series) => series.appId === "browser");
  assert.ok(codeSeries);
  assert.ok(browserSeries);

  assert.equal(chart.buckets[0].activeMinutes, 12);
  assert.ok(Math.abs(Number(chart.buckets[0][codeSeries.key]) - 41.6667) < 0.01);
  assert.ok(Math.abs(Number(chart.buckets[0][browserSeries.key]) - 58.3333) < 0.01);

  assert.equal(chart.buckets[1].activeMinutes, 7);
  assert.ok(Math.abs(Number(chart.buckets[1][codeSeries.key]) - 71.4286) < 0.01);
  assert.ok(Math.abs(Number(chart.buckets[1][browserSeries.key]) - 28.5714) < 0.01);
});
