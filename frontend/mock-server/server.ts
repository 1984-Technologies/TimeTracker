import { createServer, type IncomingMessage, type ServerResponse } from "node:http";
import type { UserAction, UserSummary } from "../src/types";

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

const actionsByUser: Record<string, UserAction[]> = {
  [users[0].id]: [
    action("3d90af82-d897-4406-ab03-fbeee2b72095", deviceIds.laptop, at(7, 40), "CollectorStarted"),
    action("7966ed44-cd50-4adc-861b-b074509d625c", deviceIds.laptop, at(7, 39), {
      AdapterStatus: {
        adapter_id: "windows-input",
        observation_type: "Keyboard",
        status: "Available",
      },
    }),
    action("5efc4120-fccb-45d4-bff9-d8b37785884f", deviceIds.laptop, at(7, 25), {
      FocusedApplicationChanged: { app_id: "code", title: "TimeTracker — Visual Studio Code" },
    }),
    action("46f780c9-83ca-4d5e-8a36-8f6636f0b315", deviceIds.laptop, at(5, 15), {
      IdleChanged: { idle: true },
    }),
    action("ce427f27-b6ee-46d0-8631-fe1f4778d41a", deviceIds.laptop, at(5, 2), {
      IdleChanged: { idle: false },
    }),
    action("9c517198-f514-4f89-a75a-c82702434e84", deviceIds.laptop, at(2, 30), {
      FocusedApplicationChanged: { app_id: "firefox", title: "Project planning" },
    }),
    action("6c7770f5-3538-4712-8154-07c74178e43f", deviceIds.laptop, at(0, 6), "Heartbeat"),
  ],
  [users[1].id]: [
    action("de971106-ea83-4038-90cc-58ddb3fb7733", deviceIds.desktop, at(9), "CollectorStarted"),
    action("61bc3339-f5d0-474b-ae92-470c819b1b4d", deviceIds.desktop, at(8, 40), {
      FocusedApplicationChanged: { app_id: "slack", title: "engineering-team" },
    }),
    action("4cb0b873-c67f-419b-836c-39d3f189bfa4", deviceIds.desktop, at(6, 5), {
      AdapterStatus: {
        adapter_id: "window-focus",
        observation_type: "FocusedApplication",
        status: { Unavailable: { reason: "Permission temporarily revoked" } },
      },
    }),
    action("f0fc09c6-9634-48fd-a4dc-e453f7c8b279", deviceIds.desktop, at(1, 15), "Heartbeat"),
  ],
  [users[2].id]: [
    action("fcda1f09-1e7c-44b7-bbdb-08196a5f0ca5", deviceIds.laptop, at(26), "CollectorStarted"),
    action("d978935c-e531-4e6b-a654-416d29bc4926", deviceIds.laptop, at(25, 48), {
      FocusedApplicationChanged: { app_id: "terminal", title: null },
    }),
    action("b5f7df29-485b-4844-8929-f11df396e141", deviceIds.laptop, at(24, 30), "CollectorStopped"),
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
