import { createHash, createHmac, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
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
    "Access-Control-Allow-Headers": "Content-Type, Accept, Authorization",
    "Access-Control-Allow-Methods": "GET, POST, PATCH, OPTIONS",
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

  const all = [...(actionsByUser[userId] ?? [])].sort((a, b) => Date.parse(a.occurred_at) - Date.parse(b.occurred_at));
  const actions = [...all.filter((item) => Date.parse(item.occurred_at) < start).slice(-100), ...all.filter((item) => Date.parse(item.occurred_at) >= start && Date.parse(item.occurred_at) <= end)];
  // Main's activity loader still expects an array until PR #47 lands.
  if (!url.searchParams.has("limit") && !url.searchParams.has("offset")) {
    sendJson(response, 200, actions); return;
  }
  const limit = Number(url.searchParams.get("limit") ?? 100), offset = Number(url.searchParams.get("offset") ?? 0);
  if (!Number.isSafeInteger(limit) || limit < 1 || limit > 1000 || !Number.isSafeInteger(offset) || offset < 0) { sendJson(response, 400, { code: "bad_request", message: "Invalid pagination" }); return; }
  sendJson(response, 200, { items: actions.slice(offset, offset + limit), total: actions.length, limit, offset });
}

// Development-only accounts; production credentials are never loaded here.
const admins = [{ id: randomUUID(), username: "admin", password: "change-me-local-only" }];
const devices = new Map<string, { device_id: string; user_id: string; credential_hash: string }>();
const key = randomBytes(32);
const ttl = Number(process.env.MOCK_TOKEN_TTL_SECONDS ?? 900);
function fail(status: number, message: string): never { throw Object.assign(new Error(message), { status }); }
function issue(sub: string, kind: "admin" | "device") {
  const iat = Math.floor(Date.now() / 1000);
  const head = Buffer.from(JSON.stringify({ alg: "HS256", typ: "JWT" })).toString("base64url");
  const body = Buffer.from(JSON.stringify({ sub, kind, iat, exp: iat + ttl, iss: "timetracker", aud: "timetracker-api" })).toString("base64url");
  const message = `${head}.${body}`;
  return { access_token: `${message}.${createHmac("sha256", key).update(message).digest("base64url")}`, token_type: "Bearer", expires_in: ttl };
}
function authenticate(request: IncomingMessage) {
  const [head, body, signature] = (request.headers.authorization?.replace(/^Bearer /, "") ?? "").split(".");
  const expected = createHmac("sha256", key).update(`${head}.${body}`).digest("base64url");
  if (!signature || signature.length !== expected.length || !timingSafeEqual(Buffer.from(signature), Buffer.from(expected))) fail(401, "Invalid credentials or token");
  const claims = JSON.parse(Buffer.from(body, "base64url").toString()) as { sub: string; kind: string; exp: number };
  if (claims.exp <= Date.now() / 1000) fail(401, "Token expired");
  if (claims.kind !== "admin") fail(403, "Administrator token required");
  const admin = admins.find((admin) => admin.id === claims.sub);
  if (!admin) fail(401, "Unknown administrator");
  return admin;
}
async function readBody(request: IncomingMessage): Promise<Record<string, string>> {
  let text = "";
  for await (const chunk of request) { text += String(chunk); if (text.length > 16384) fail(400, "Request too large"); }
  try { const value: unknown = JSON.parse(text); if (!value || typeof value !== "object" || Array.isArray(value)) fail(400, "Invalid JSON"); return value as Record<string, string>; }
  catch { fail(400, "Invalid JSON request"); }
}
function name(value: unknown) { if (typeof value !== "string" || !value.trim() || value.length > 200) fail(400, "Name is required"); return value.trim(); }
function password(value: unknown) { if (typeof value !== "string" || !value || value.length > 1024) fail(400, "Password is required"); return value; }
function deviceCheck(data: Record<string, string>) {
  if (typeof data.secret !== "string" || !/^[A-Za-z0-9_-]{43}$/.test(data.secret)) fail(400, "Invalid device secret");
  const raw = Buffer.from(data.secret, "base64url");
  if (raw.length !== 32 || raw.toString("base64url") !== data.secret) fail(400, "Invalid device secret");
  const device = devices.get(data.device_id), hash = createHash("sha256").update(raw).digest("hex");
  if (device && !timingSafeEqual(Buffer.from(hash), Buffer.from(device.credential_hash))) fail(401, "Invalid credentials");
  return device;
}
const server = createServer((request, response) => { void (async () => {
  const send = (status: number, value: unknown) => sendJson(response, status, value);
  if (request.method === "OPTIONS") { send(204, null); return; }
  const url = new URL(request.url ?? "/", `http://localhost:${port}`), path = url.pathname, method = request.method;
  if (path === "/health") { send(200, { ok: true }); return; }
  if (method === "POST" && path === "/api/auth/login") {
    const data = await readBody(request), admin = admins.find((admin) => admin.username === data.username && admin.password === data.password);
    if (!admin) fail(401, "Invalid credentials");
    send(200, issue(admin.id, "admin")); return;
  }
  if (method === "POST" && ["/api/devices/register", "/api/devices/login"].includes(path)) {
    const data = await readBody(request), device = deviceCheck(data);
    if (path.endsWith("register")) send(200, { registered: Boolean(device) });
    else { if (!device) fail(401, "Device is not approved"); send(200, issue(device.device_id, "device")); }
    return;
  }
  const principal = authenticate(request);
  if (path === "/api/auth/me" && method === "GET") { send(200, { id: principal.id, username: principal.username }); return; }
  if (path === "/api/admins") {
    if (method === "GET") send(200, admins.map(({ id, username }) => ({ id, username })));
    else if (method === "POST") {
      const data = await readBody(request), username = name(data.username);
      if (admins.some((admin) => admin.username === username)) fail(409, "Username already exists");
      const admin = { id: randomUUID(), username, password: password(data.password) }; admins.push(admin); send(201, { id: admin.id, username });
    } else fail(405, "Method not allowed");
    return;
  }
  const adminPath = path.match(/^\/api\/admins\/([^/]+)$/);
  if (adminPath && method === "PATCH") {
    const admin = admins.find((admin) => admin.id === adminPath[1]); if (!admin) fail(404, "Admin not found");
    admin.password = password((await readBody(request)).password); send(204, null); return;
  }
  if (path === "/users") {
    if (method === "GET") send(200, users);
    else if (method === "POST") { const user = { id: randomUUID(), name: name((await readBody(request)).name) }; users.push(user); send(201, user); }
    else fail(405, "Method not allowed");
    return;
  }
  if (path === "/api/devices") {
    if (method === "GET") send(200, [...devices.values()].map(({ device_id, user_id }) => ({ device_id, user_id })));
    else if (method === "POST") {
      const data = await readBody(request);
      if (!/^[0-9a-f]{8}(-[0-9a-f]{4}){3}-[0-9a-f]{12}$/i.test(data.device_id ?? "") || !/^[0-9a-f]{64}$/.test(data.credential_hash ?? "")) fail(400, "Invalid device ID or hash");
      if (!users.some((user) => user.id === data.user_id)) fail(404, "User not found");
      const old = devices.get(data.device_id);
      if (old && (old.user_id !== data.user_id || old.credential_hash !== data.credential_hash)) fail(409, "Device approval conflict");
      devices.set(data.device_id, { device_id: data.device_id, user_id: data.user_id, credential_hash: data.credential_hash }); send(200, { device_id: data.device_id, user_id: data.user_id });
    } else fail(405, "Method not allowed");
    return;
  }
  const devicePath = path.match(/^\/api\/devices\/([^/]+)$/);
  if (devicePath && method === "GET") {
    const device = devices.get(devicePath[1]); if (!device) fail(404, "Device not found");
    send(200, { device_id: device.device_id, user_id: device.user_id }); return;
  }
  const actionsPath = path.match(/^\/users\/([^/]+)\/actions$/);
  if (actionsPath && method === "GET") { handleActions(request, response, url, decodeURIComponent(actionsPath[1])); return; }
  fail(404, "Route not found");
})().catch((error: unknown) => {
  const value = error as { status?: number; message?: string }, status = value.status ?? 500;
  sendJson(response, status, { code: ({400:"bad_request",401:"unauthorized",403:"forbidden",404:"not_found",409:"conflict"} as Record<number,string>)[status] ?? "internal_error", message: status === 500 ? "Internal server error" : value.message });
}); });
server.listen(port, "127.0.0.1", () => {
  const address = server.address();
  console.log(`TimeTracker mock API listening at http://localhost:${typeof address === "object" && address ? address.port : port}`);
});
