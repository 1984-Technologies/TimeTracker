import { test } from "node:test";
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { createHash } from "node:crypto";
import { resolve } from "node:path";

test("mock supports JWT login, device approval, errors, pagination, and expiry", async (test) => {
  const child = spawn(process.execPath, ["--import", "tsx", "mock-server/server.ts"], {
    cwd: resolve(import.meta.dirname, ".."), env: { ...process.env, MOCK_PORT: "0", MOCK_TOKEN_TTL_SECONDS: "2" }, stdio: ["ignore", "pipe", "pipe"],
  });
  test.after(() => { child.kill(); });
  const origin = await new Promise<string>((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error("Mock did not start")), 8000);
    child.on("error", reject);
    child.on("exit", (code) => { clearTimeout(timeout); if (code) reject(new Error(`Mock exited ${code}`)); });
    child.stdout.on("data", (chunk: Buffer) => {
      const match = String(chunk).match(/http:\/\/localhost:\d+/);
      if (match) { clearTimeout(timeout); resolve(match[0]); }
    });
  });
  const login = await fetch(`${origin}/api/auth/login`, { method:"POST", headers:{"Content-Type":"application/json"}, body:JSON.stringify({username:"admin",password:"change-me-local-only"}) });
  assert.equal(login.status,200);
  const { access_token } = await login.json() as { access_token:string };
  const headers = { Authorization:`Bearer ${access_token}`, "Content-Type":"application/json" };
  assert.equal((await fetch(`${origin}/users`)).status,401);
  const users = await (await fetch(`${origin}/users`,{headers})).json() as {id:string}[];
  const credentials={device_id:"00000000-0000-4000-8000-000000000123",secret:Buffer.alloc(32,42).toString("base64url")};
  const post = (path:string,body:unknown,authorized=true) => fetch(`${origin}${path}`,{method:"POST",headers:authorized?headers:{"Content-Type":"application/json"},body:JSON.stringify(body)});
  assert.deepEqual(await (await post("/api/devices/register",credentials,false)).json(),{registered:false});
  const approval={device_id:credentials.device_id,user_id:users[0].id,credential_hash:createHash("sha256").update(Buffer.alloc(32,42)).digest("hex")};
  assert.equal((await post("/api/devices",approval)).status,200);
  assert.deepEqual(await (await post("/api/devices/register",credentials,false)).json(),{registered:true});
  assert.equal((await post("/api/devices",{...approval,user_id:users[1].id})).status,409);
  const devices=await (await fetch(`${origin}/api/devices`,{headers})).json() as object[];
  assert.equal("credential_hash" in devices[0],false);
  const legacy=await (await fetch(`${origin}/users/${users[0].id}/actions?start=2000-01-01&end=2100-01-01`,{headers})).json();
  assert(Array.isArray(legacy));assert(legacy.length>1);
  const page=await (await fetch(`${origin}/users/${users[0].id}/actions?start=2000-01-01&end=2100-01-01&limit=1`,{headers})).json() as {items:unknown[];total:number};
  assert.equal(page.items.length,1);assert(page.total>1);
  const created=await post("/api/admins",{username:"second",password:"initial"});assert.equal(created.status,201);
  const admin=await created.json() as {id:string};
  assert.equal((await fetch(`${origin}/api/admins/${admin.id}`,{method:"PATCH",headers,body:JSON.stringify({password:"updated"})})).status,204);
  await new Promise((resolve)=>setTimeout(resolve,2100));
  assert.equal((await fetch(`${origin}/users`,{headers})).status,401);
});
