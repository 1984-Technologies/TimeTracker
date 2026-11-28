/// <reference types="vite/client" />
import { afterEach, test } from "node:test";
import assert from "node:assert/strict";
import { parseEnrollment } from "../src/enrollment";
import { createServer } from "vite";

// Load browser modules with Vite, keeping the config hotfix owned by PR #47.
const vite = await createServer({ configFile: false, optimizeDeps: { noDiscovery: true }, server: { middlewareMode: true, hmr: false, watch: null } });
const { clearSession, getToken, sessionSignal, setSession } = await vite.ssrLoadModule("/src/session.ts") as typeof import("../src/session");
const { ApiError, changePassword, getUsers } = await vite.ssrLoadModule("/src/api.ts") as typeof import("../src/api");
await vite.close();

const originalFetch = globalThis.fetch;
afterEach(() => { globalThis.fetch = originalFetch; clearSession(); });
const expected = "http://localhost:5173/devices/register";
const id = "00000000-0000-4000-8000-000000000001";
const link = `${expected}#device_id=${id}&credential_hash=${"a".repeat(64)}`;

test("enrollment accepts only this dashboard's exact path and valid fragment", () => {
  assert.deepEqual(parseEnrollment(link, expected), { device_id: id, credential_hash: "a".repeat(64) });
  for (const value of [link.replace("localhost", "evil.example"), link.replace("/devices/register", "/other"), link.replace("#", "?leak=true#"), link.replace("device_id=", "secret="), link.replace(id,"bad"), link.replace("credential_hash=a", "credential_hash=A"), `${link}&device_id=${id}`, link.replace("http://", "file://"), link.replace("localhost", "user:password@localhost")]) {
    assert.throws(() => parseEnrollment(value, expected), Error, value);
  }
});

test("private requests carry bearer headers and JSON mutations accept 204", async () => {
  setSession({access_token:"token",token_type:"Bearer",expires_in:900});
  const calls: {url:string;init?:RequestInit}[]=[];
  globalThis.fetch = async (input, init) => {
    calls.push({url:String(input),init});
    return init?.method === "PATCH" ? new Response(null,{status:204}) : Response.json([]);
  };
  assert.deepEqual(await getUsers(),[]);
  assert.equal((calls[0].init?.headers as Record<string,string>).Authorization,"Bearer token");
  assert.equal(await changePassword(id,"updated"),undefined);
  assert.equal(calls.at(-1)?.init?.method,"PATCH");
  assert.deepEqual(JSON.parse(String(calls.at(-1)?.init?.body)),{password:"updated"});
});

test("logout, expiry and 401 discard the token and abort stale work", async () => {
  setSession({access_token:"token",token_type:"Bearer",expires_in:900});
  const old=sessionSignal();clearSession();assert(old.aborted);assert.equal(getToken(),null);
  setSession({access_token:"token",token_type:"Bearer",expires_in:0.01});
  await new Promise((resolve)=>setTimeout(resolve,25));assert.equal(getToken(),null);
  setSession({access_token:"new-token",token_type:"Bearer",expires_in:900});
  const signal=sessionSignal();globalThis.fetch=async()=>Response.json({message:"Expired"},{status:401});
  await assert.rejects(getUsers(),(error:unknown)=>error instanceof ApiError && error.status===401);
  assert.equal(getToken(),null);assert(signal.aborted);
});
