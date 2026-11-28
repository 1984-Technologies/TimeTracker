import { useEffect, useState, useSyncExternalStore, type FormEvent, type ReactNode } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { login } from "./api";
import { getToken, setSession, subscribeSession } from "./session";

export function AuthGate({ children }: { children: ReactNode }) {
  const token = useSyncExternalStore(subscribeSession, getToken);
  const cache = useQueryClient();
  useEffect(() => subscribeSession(() => { void cache.cancelQueries(); cache.clear(); }), [cache]);
  return token ? children : <LoginPage />;
}
function LoginPage() {
  const [error, setError] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const data = new FormData(event.currentTarget);
    setBusy(true); setError("");
    try { setSession(await login(String(data.get("username")), String(data.get("password")))); }
    catch (error) { setError(String(error instanceof Error ? error.message : error)); }
    finally { setBusy(false); }
  }
  return <main className="mx-auto max-w-md px-6 py-20">
    <h1 className="text-3xl font-semibold">Sign in to TimeTracker</h1>
    <p className="my-5 text-slate-500">Use an administrator account. Device registration links are preserved while you sign in.</p>
    <form onSubmit={submit} className="space-y-4">
      <label className="block">Username<input name="username" autoComplete="username" required maxLength={200} className="auth-input" /></label>
      <label className="block">Password<input name="password" type="password" autoComplete="current-password" required maxLength={1024} className="auth-input" /></label>
      <button disabled={busy} className="auth-button">{busy ? "Signing in…" : "Sign in"}</button>
      {error && <p role="alert" className="text-red-700">{error}</p>}
    </form>
  </main>;
}
