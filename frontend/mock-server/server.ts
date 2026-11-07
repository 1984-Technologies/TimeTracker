import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type {
  AdapterStatus,
  ObservationType,
  UserAction,
  UserSummary,
} from "../src/types";

const port = Number(process.env.MOCK_PORT ?? 8239);

const users: UserSummary[] = [
  { id: "0b8dc9ef-6064-4b8e-af41-56a00b6a1d16", name: "Alex Morgan" },
  { id: "ed7f2f28-c6d1-428f-a97b-b99361b91474", name: "Sam Rivera" },
  { id: "de63e850-9bf5-4b6a-9d66-95cc667355d4", name: "Jordan Lee" },
];

const deviceIds = {
  laptop: "f1253523-8f50-4348-87c3-a8e71ef5bb32",
  desktop: "01ecc0c1-cd26-445b-86dd-f76e1d1e01f1",
};

function at(hoursAgo: number, minutesAgo = 0): string {
  return new Date(Date.now() - (hoursAgo * 60 + minutesAgo) * 60_000).toISOString();
}

function action(
  id: string,
  deviceId: string,
  occurredAt: string,
  kind: UserAction["kind"],
): UserAction {
  return { id, device_id: deviceId, occurred_at: occurredAt, kind };
}

let eventSequence = 0;

function nextEventId(): string {
  eventSequence += 1;
  return `00000000-0000-4000-8000-${eventSequence.toString().padStart(12, "0")}`;
}

function simpleAction(
  deviceId: string,
  hoursAgo: number,
  minutesAgo: number,
  kind: "CollectorStarted" | "CollectorStopped" | "Heartbeat",
): UserAction {
  return action(nextEventId(), deviceId, at(hoursAgo, minutesAgo), kind);
}

function focusedApp(
  deviceId: string,
  hoursAgo: number,
  minutesAgo: number,
  appId: string,
  title: string | null,
): UserAction {
  return action(nextEventId(), deviceId, at(hoursAgo, minutesAgo), {
    FocusedApplicationChanged: { app_id: appId, title },
  });
}

function idleChanged(
  deviceId: string,
  hoursAgo: number,
  minutesAgo: number,
  idle: boolean,
): UserAction {
  return action(nextEventId(), deviceId, at(hoursAgo, minutesAgo), {
    IdleChanged: { idle },
  });
}

function adapterStatus(
  deviceId: string,
  hoursAgo: number,
  minutesAgo: number,
  adapterId: string,
  observationType: ObservationType,
  status: AdapterStatus,
): UserAction {
  return action(nextEventId(), deviceId, at(hoursAgo, minutesAgo), {
    AdapterStatus: {
      adapter_id: adapterId,
      observation_type: observationType,
      status,
    },
  });
}

const actionsByUser: Record<string, UserAction[]> = {
  [users[0].id]: [
    simpleAction(deviceIds.laptop, 7, 50, "CollectorStarted"),
    adapterStatus(
      deviceIds.laptop,
      7,
      47,
      "windows-keyboard",
      "Keyboard",
      { Unavailable: { reason: "Driver was still starting" } },
    ),
    adapterStatus(deviceIds.laptop, 7, 44, "windows-keyboard", "Keyboard", "Available"),
    adapterStatus(deviceIds.laptop, 7, 43, "window-focus", "FocusedApplication", "Available"),
    adapterStatus(deviceIds.laptop, 7, 42, "idle-monitor", "Idle", "Available"),
    adapterStatus(deviceIds.laptop, 7, 41, "pointer-click", "MouseClick", "Available"),
    adapterStatus(deviceIds.laptop, 7, 40, "pointer-motion", "MouseMove", "Available"),
    focusedApp(deviceIds.laptop, 7, 35, "code", "TimeTracker — Visual Studio Code"),
    focusedApp(deviceIds.laptop, 6, 58, "firefox", "API documentation"),
    focusedApp(deviceIds.laptop, 6, 43, "code", "UserPage.tsx — TimeTracker"),
    focusedApp(deviceIds.laptop, 6, 35, "slack", "engineering-team"),
    focusedApp(deviceIds.laptop, 6, 18, "code", "analytics.ts — TimeTracker"),
    idleChanged(deviceIds.laptop, 5, 15, true),
    idleChanged(deviceIds.laptop, 5, 2, false),
    focusedApp(deviceIds.laptop, 4, 48, "terminal", "npm run dev"),
    focusedApp(deviceIds.laptop, 4, 37, "code", "SensorStatusGrid.tsx — TimeTracker"),
    focusedApp(deviceIds.laptop, 4, 12, "notion", "Sprint plan"),
    focusedApp(deviceIds.laptop, 3, 55, "firefox", "Pull request #21"),
    focusedApp(deviceIds.laptop, 3, 47, "slack", "frontend-team"),
    focusedApp(deviceIds.laptop, 3, 34, "code", "AppActivityChart.tsx — TimeTracker"),
    focusedApp(deviceIds.laptop, 2, 30, "firefox", "Project planning"),
    focusedApp(deviceIds.laptop, 2, 18, "code", "UserPage.tsx — TimeTracker"),
    focusedApp(deviceIds.laptop, 2, 5, "slack", "design-review"),
    focusedApp(deviceIds.laptop, 1, 57, "firefox", "Recharts documentation"),
    focusedApp(deviceIds.laptop, 1, 43, "code", "analytics.ts — TimeTracker"),
    focusedApp(deviceIds.laptop, 1, 30, "terminal", "npm run build"),
    focusedApp(deviceIds.laptop, 1, 20, "code", "TimeTracker — Visual Studio Code"),
    focusedApp(deviceIds.laptop, 0, 58, "firefox", "Pull request #21"),
    focusedApp(deviceIds.laptop, 0, 52, "slack", "frontend-team"),
    focusedApp(deviceIds.laptop, 0, 44, "code", "TimeTracker — Visual Studio Code"),
    idleChanged(deviceIds.laptop, 0, 30, true),
    adapterStatus(
      deviceIds.laptop,
      0,
      27,
      "pointer-click",
      "MouseClick",
      { Unavailable: { reason: "Accessibility permission was revoked" } },
    ),
    idleChanged(deviceIds.laptop, 0, 18, false),
    focusedApp(deviceIds.laptop, 0, 14, "terminal", "git status"),
    simpleAction(deviceIds.laptop, 0, 6, "Heartbeat"),
  ],
  [users[1].id]: [
    simpleAction(deviceIds.desktop, 9, 0, "CollectorStarted"),
    adapterStatus(deviceIds.desktop, 8, 58, "x11-focus", "FocusedApplication", "Available"),
    adapterStatus(deviceIds.desktop, 8, 57, "x11-idle", "Idle", "Available"),
    adapterStatus(deviceIds.desktop, 8, 56, "evdev-keyboard", "Keyboard", "Available"),
    focusedApp(deviceIds.desktop, 8, 40, "slack", "engineering-team"),
    focusedApp(deviceIds.desktop, 7, 30, "idea", "Backend — IntelliJ IDEA"),
    focusedApp(deviceIds.desktop, 6, 42, "firefox", "Axum documentation"),
    focusedApp(deviceIds.desktop, 6, 33, "idea", "dashboard.rs — Backend"),
    idleChanged(deviceIds.desktop, 4, 20, true),
    idleChanged(deviceIds.desktop, 4, 5, false),
    focusedApp(deviceIds.desktop, 2, 18, "terminal", "cargo test"),
    focusedApp(deviceIds.desktop, 2, 7, "idea", "Backend — IntelliJ IDEA"),
    simpleAction(deviceIds.desktop, 1, 15, "Heartbeat"),
  ],
  [users[2].id]: [
    simpleAction(deviceIds.laptop, 6, 10, "CollectorStarted"),
    adapterStatus(deviceIds.laptop, 6, 8, "macos-focus", "FocusedApplication", "Available"),
    adapterStatus(
      deviceIds.laptop,
      6,
      7,
      "macos-keyboard",
      "Keyboard",
      { Unavailable: { reason: "Input monitoring permission is missing" } },
    ),
    focusedApp(deviceIds.laptop, 6, 0, "terminal", null),
    focusedApp(deviceIds.laptop, 5, 25, "safari", "Team calendar"),
    focusedApp(deviceIds.laptop, 5, 12, "terminal", "tmux"),
    focusedApp(deviceIds.laptop, 3, 40, "figma", "TimeTracker dashboard"),
    focusedApp(deviceIds.laptop, 3, 27, "safari", "Design system"),
    simpleAction(deviceIds.laptop, 0, 45, "CollectorStopped"),
  ],
};

function sendJson(response: ServerResponse, status: number, body: unknown): void {
  response.writeHead(status, {
    "Access-Control-Allow-Headers": "Content-Type, Accept",
    "Access-Control-Allow-Methods": "GET, OPTIONS",
    "Access-Control-Allow-Origin": "*",
    "Content-Type": "application/json; charset=utf-8",
  });
  response.end(JSON.stringify(body));
}

function handleActions(
  request: IncomingMessage,
  response: ServerResponse,
  url: URL,
  userId: string,
): void {
  if (!users.some((user) => user.id === userId)) {
    sendJson(response, 404, { message: "User not found" });
    return;
  }

  const startValue = url.searchParams.get("start");
  const endValue = url.searchParams.get("end");
  const start = startValue ? Date.parse(startValue) : Number.NaN;
  const end = endValue ? Date.parse(endValue) : Number.NaN;

  if (!Number.isFinite(start) || !Number.isFinite(end) || start > end) {
    sendJson(response, 400, { message: "Valid start and end query parameters are required" });
    return;
  }

  const actions = (actionsByUser[userId] ?? [])
    .filter((item) => {
      const timestamp = Date.parse(item.occurred_at);
      return timestamp >= start && timestamp <= end;
    })
    .sort((left, right) => Date.parse(right.occurred_at) - Date.parse(left.occurred_at));

  sendJson(response, 200, actions);
}

const server = createServer((request, response) => {
  if (request.method === "OPTIONS") {
    sendJson(response, 204, null);
    return;
  }

  if (request.method !== "GET" || !request.url) {
    sendJson(response, 405, { message: "Method not allowed" });
    return;
  }

  const url = new URL(request.url, `http://${request.headers.host ?? `localhost:${port}`}`);
  if (url.pathname === "/users") {
    sendJson(response, 200, users);
    return;
  }

  const match = url.pathname.match(/^\/users\/([^/]+)\/actions$/);
  if (match) {
    handleActions(request, response, url, decodeURIComponent(match[1]));
    return;
  }

  sendJson(response, 404, { message: "Route not found" });
});

server.listen(port, "127.0.0.1", () => {
  console.log(`TimeTracker mock API listening at http://localhost:${port}`);
});
