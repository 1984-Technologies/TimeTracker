import type { ObservationType, UserAction, UserActionKind } from "./types";

export const eventKindOptions = [
  { value: "CollectorStarted", label: "Collector started" },
  { value: "CollectorStopped", label: "Collector stopped" },
  { value: "Heartbeat", label: "Heartbeat" },
  { value: "IdleChanged", label: "Idle state changed" },
  { value: "FocusedApplicationChanged", label: "Application changed" },
  { value: "KeyboardButtonPressed", label: "Keyboard input" },
  { value: "MouseClicked", label: "Mouse click" },
  { value: "MouseMoved", label: "Mouse movement" },
  { value: "SystemSleepChanged", label: "System sleep state" },
  { value: "AdapterStatus", label: "Adapter status" },
] as const;

export type EventKindName = (typeof eventKindOptions)[number]["value"];

export function actionKindName(kind: UserActionKind): EventKindName {
  if (typeof kind === "string") return kind;
  if ("IdleChanged" in kind) return "IdleChanged";
  if ("FocusedApplicationChanged" in kind) return "FocusedApplicationChanged";
  if ("KeyboardButtonPressed" in kind) return "KeyboardButtonPressed";
  if ("MouseClicked" in kind) return "MouseClicked";
  if ("MouseMoved" in kind) return "MouseMoved";
  if ("SystemSleepChanged" in kind) return "SystemSleepChanged";
  return "AdapterStatus";
}

export function sortActionsNewestFirst(actions: UserAction[]): UserAction[] {
  return [...actions].sort(
    (left, right) => Date.parse(right.occurred_at) - Date.parse(left.occurred_at),
  );
}

export interface SensorStatus {
  observationType: ObservationType;
  adapterId: string;
  available: boolean;
  reason?: string;
  occurredAt: string;
  deviceId: string;
}

export function deriveSensorStatuses(actions: UserAction[]): SensorStatus[] {
  const latestByType = new Map<ObservationType, SensorStatus>();

  for (const action of sortActionsNewestFirst(actions)) {
    if (typeof action.kind === "string" || !("AdapterStatus" in action.kind)) continue;

    const adapter = action.kind.AdapterStatus;
    if (latestByType.has(adapter.observation_type)) continue;

    latestByType.set(adapter.observation_type, {
      observationType: adapter.observation_type,
      adapterId: adapter.adapter_id,
      available: adapter.status === "Available",
      reason:
        adapter.status === "Available" ? undefined : adapter.status.Unavailable.reason,
      occurredAt: action.occurred_at,
      deviceId: action.device_id,
    });
  }

  return [...latestByType.values()].sort((left, right) =>
    left.observationType.localeCompare(right.observationType),
  );
}

const BUCKET_MS = 15 * 60 * 1000;
const appColors = [
  "#237a63",
  "#4f6fa8",
  "#d28a3c",
  "#8b65a5",
  "#c75d69",
  "#4d8d9b",
  "#7b8c45",
  "#9a6a4a",
];

export interface AppSeries {
  key: string;
  appId: string;
  color: string;
  totalMilliseconds: number;
}

export interface AppActivityBucket {
  timestamp: number;
  label: string;
  tooltipLabel: string;
  activeMinutes: number;
  [key: string]: number | string;
}

export interface AppActivityChartData {
  buckets: AppActivityBucket[];
  series: AppSeries[];
  totalActiveMilliseconds: number;
}

function addActiveInterval(
  bucketDurations: Map<number, Map<string, number>>,
  appTotals: Map<string, number>,
  appId: string,
  start: number,
  end: number,
): void {
  let cursor = start;

  while (cursor < end) {
    const bucketStart = Math.floor(cursor / BUCKET_MS) * BUCKET_MS;
    const segmentEnd = Math.min(end, bucketStart + BUCKET_MS);
    const duration = segmentEnd - cursor;
    const durations = bucketDurations.get(bucketStart);

    if (durations) {
      durations.set(appId, (durations.get(appId) ?? 0) + duration);
      appTotals.set(appId, (appTotals.get(appId) ?? 0) + duration);
    }

    cursor = segmentEnd;
  }
}

function bucketLabels(timestamp: number, rangeMilliseconds: number) {
  const start = new Date(timestamp);
  const end = new Date(timestamp + BUCKET_MS);
  const timeFormatter = new Intl.DateTimeFormat(undefined, {
    hour: "2-digit",
    minute: "2-digit",
  });
  const axisFormatter = new Intl.DateTimeFormat(undefined, {
    ...(rangeMilliseconds > 24 * 60 * 60 * 1000
      ? { month: "short" as const, day: "numeric" as const }
      : {}),
    hour: "2-digit",
    minute: "2-digit",
  });
  const dateFormatter = new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
  });

  return {
    label: axisFormatter.format(start),
    tooltipLabel: `${dateFormatter.format(start)}, ${timeFormatter.format(start)}–${timeFormatter.format(end)}`,
  };
}

export function buildAppActivityChart(
  actions: UserAction[],
  rangeStart: string,
  rangeEnd: string,
): AppActivityChartData {
  const start = Date.parse(rangeStart);
  const end = Date.parse(rangeEnd);

  if (!Number.isFinite(start) || !Number.isFinite(end) || start >= end) {
    return { buckets: [], series: [], totalActiveMilliseconds: 0 };
  }

  const firstBucket = Math.floor(start / BUCKET_MS) * BUCKET_MS;
  const bucketDurations = new Map<number, Map<string, number>>();
  for (let timestamp = firstBucket; timestamp < end; timestamp += BUCKET_MS) {
    bucketDurations.set(timestamp, new Map());
  }

  const appTotals = new Map<string, number>();
  const orderedActions = [...actions]
    .filter((action) => {
      const timestamp = Date.parse(action.occurred_at);
      return timestamp >= start && timestamp <= end;
    })
    .sort((left, right) => Date.parse(left.occurred_at) - Date.parse(right.occurred_at));

  let cursor = start;
  let activeApp: string | null = null;
  let idle = false;
  let collectorRunning = true;

  for (const action of orderedActions) {
    const timestamp = Math.min(end, Math.max(start, Date.parse(action.occurred_at)));
    if (activeApp && !idle && collectorRunning && timestamp > cursor) {
      addActiveInterval(bucketDurations, appTotals, activeApp, cursor, timestamp);
    }
    cursor = timestamp;

    if (action.kind === "CollectorStarted") {
      collectorRunning = true;
    } else if (action.kind === "CollectorStopped") {
      collectorRunning = false;
      activeApp = null;
    } else if (typeof action.kind !== "string" && "IdleChanged" in action.kind) {
      idle = action.kind.IdleChanged.idle;
    } else if (
      typeof action.kind !== "string" &&
      "FocusedApplicationChanged" in action.kind
    ) {
      activeApp = action.kind.FocusedApplicationChanged.app_id;
    }
  }

  if (activeApp && !idle && collectorRunning && cursor < end) {
    addActiveInterval(bucketDurations, appTotals, activeApp, cursor, end);
  }

  const series = [...appTotals.entries()]
    .sort((left, right) => right[1] - left[1])
    .map(([appId, totalMilliseconds], index) => ({
      key: `app${index}`,
      appId,
      color: appColors[index % appColors.length],
      totalMilliseconds,
    }));
  const seriesByApp = new Map(series.map((item) => [item.appId, item]));
  const rangeMilliseconds = end - start;

  const buckets = [...bucketDurations.entries()].map(([timestamp, durations]) => {
    const total = [...durations.values()].reduce((sum, duration) => sum + duration, 0);
    const labels = bucketLabels(timestamp, rangeMilliseconds);
    const bucket: AppActivityBucket = {
      timestamp,
      label: labels.label,
      tooltipLabel: labels.tooltipLabel,
      activeMinutes: total / 60_000,
    };

    for (const [appId, duration] of durations) {
      const item = seriesByApp.get(appId);
      if (item && total > 0) bucket[item.key] = (duration / total) * 100;
    }

    return bucket;
  });

  return {
    buckets,
    series,
    totalActiveMilliseconds: [...appTotals.values()].reduce(
      (sum, duration) => sum + duration,
      0,
    ),
  };
}

export function formatDuration(milliseconds: number): string {
  const totalMinutes = Math.round(milliseconds / 60_000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours === 0) return `${minutes}m`;
  return minutes === 0 ? `${hours}h` : `${hours}h ${minutes}m`;
}
