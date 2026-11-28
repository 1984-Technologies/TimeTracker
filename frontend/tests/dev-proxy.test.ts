import assert from "node:assert/strict";
import { createServer as createBackend } from "node:http";
import { resolve } from "node:path";
import { test } from "node:test";
import { createServer, resolveConfig } from "vite";

test("real-mode login stays same-origin and preserves backend status and bearer headers", async (test) => {
  const root = resolve(import.meta.dirname, "..");
  const config = await resolveConfig({ root, logLevel: "silent" }, "serve", "real");
  assert.equal(config.env.VITE_API_BASE_URL, "/backend");
  const proxy = config.server.proxy?.["/backend"];
  assert(proxy && typeof proxy !== "string");
  assert.equal(proxy.target, "http://127.0.0.1:8239");
  const backend = createBackend((request, response) => {
    response.writeHead(401, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ path: request.url, method: request.method, authorization: request.headers.authorization }));
  });
  test.after(() => { backend.closeAllConnections(); backend.close(); });
  await new Promise<void>((resolve) => backend.listen(0, "127.0.0.1", resolve));
  const backendAddress = backend.address();
  assert(backendAddress && typeof backendAddress !== "string");
  const vite = await createServer({
    configFile: false, root, logLevel: "silent", appType: "custom",
    optimizeDeps: { noDiscovery: true },
    server: { host: "127.0.0.1", port: 0, hmr: false, watch: null,
      proxy: { "/backend": { ...proxy, target: `http://127.0.0.1:${backendAddress.port}` } } },
  });
  test.after(() => vite.close());
  await vite.listen();
  const address = vite.httpServer?.address();
  assert(address && typeof address !== "string");
  const origin = `http://127.0.0.1:${address.port}`;
  const response = await fetch(`${origin}${config.env.VITE_API_BASE_URL}/api/auth/login`, {
    method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer test-token", Origin: origin },
    body: JSON.stringify({ username: "invalid", password: "invalid" }),
  });
  assert.equal(new URL(response.url).origin, origin);
  assert.equal(response.status, 401);
  assert.deepEqual(await response.json(), { path: "/api/auth/login", method: "POST", authorization: "Bearer test-token" });
});
