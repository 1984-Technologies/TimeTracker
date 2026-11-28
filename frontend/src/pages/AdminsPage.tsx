import { useState, type FormEvent } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { changePassword, createAdmin, currentAdmin, getAdmins } from "../api";
import { ErrorState } from "../components/Feedback";
import { clearSession } from "../session";

export function AdminsPage() {
  const cache = useQueryClient();
  const admins = useQuery({ queryKey: ["admins"], queryFn: ({ signal }) => getAdmins(signal) });
  const me = useQuery({ queryKey: ["me"], queryFn: ({ signal }) => currentAdmin(signal) });
  const [message, setMessage] = useState("");
  const [busy, setBusy] = useState(false);
  async function submit(event: FormEvent<HTMLFormElement>, id?: string) {
    event.preventDefault(); const form = event.currentTarget; const values = new FormData(form); setBusy(true); setMessage("");
    try {
      if (id) { await changePassword(id, String(values.get("password"))); if (id === me.data?.id) { clearSession(); return; } }
      else await createAdmin(String(values.get("username")), String(values.get("password")));
      form.reset(); await cache.invalidateQueries({ queryKey: ["admins"] }); setMessage(id ? "Password changed." : "Administrator created.");
    } catch (error) { setMessage((error as Error).message); }
    finally { setBusy(false); }
  }
  return <section className="space-y-6">
    <h1 className="text-3xl font-semibold">Administrators</h1>
    <form className="auth-panel space-y-3" onSubmit={(event) => void submit(event)}><h2 className="text-xl font-semibold">Create administrator</h2>
      <label className="block">Username<input className="auth-input" name="username" required maxLength={200} autoComplete="off" /></label>
      <label className="block">Password<input className="auth-input" type="password" name="password" required maxLength={1024} autoComplete="new-password" /></label>
      <button className="auth-button" disabled={busy}>Create administrator</button>
    </form>
    {message && <p role="status">{message}</p>}
    {admins.isPending ? <p>Loading administrators…</p> : admins.isError ? <ErrorState error={admins.error} /> : admins.data.map((admin) => <form key={admin.id} className="auth-panel flex flex-wrap items-end gap-3" onSubmit={(event) => void submit(event, admin.id)}>
      <label className="grow">New password for {admin.username}<input className="auth-input" name="password" type="password" required maxLength={1024} autoComplete="new-password" /></label><button className="auth-secondary" disabled={busy}>Change password</button>
    </form>)}
  </section>;
}
