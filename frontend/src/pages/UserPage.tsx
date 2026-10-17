import { useQuery } from "@tanstack/react-query";
import { ArrowLeft, CalendarRange, Laptop2, Radio } from "lucide-react";
import { useMemo, useState } from "react";
import { Link, useParams } from "react-router-dom";
import { getUserActions, getUsers } from "../api";
import { ErrorState, LoadingState } from "../components/Feedback";
import { formatDateTime, presentActionKind, toDateTimeLocal } from "../lib";

const toneClasses = {
  green: "bg-emerald-50 text-emerald-700 ring-emerald-600/10",
  amber: "bg-amber-50 text-amber-700 ring-amber-600/10",
  slate: "bg-slate-100 text-slate-600 ring-slate-500/10",
  red: "bg-red-50 text-red-700 ring-red-600/10",
} as const;

function initialRange() {
  const end = new Date();
  const start = new Date(end.getTime() - 24 * 60 * 60 * 1000);
  return { start: toDateTimeLocal(start), end: toDateTimeLocal(end) };
}

export function UserPage() {
  const { userId = "" } = useParams();
  const [draftRange, setDraftRange] = useState(initialRange);
  const [range, setRange] = useState(initialRange);

  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => getUsers(signal),
  });
  const actions = useQuery({
    queryKey: ["actions", userId, range.start, range.end],
    queryFn: ({ signal }) =>
      getUserActions(
        userId,
        {
          start: new Date(range.start).toISOString(),
          end: new Date(range.end).toISOString(),
        },
        signal,
      ),
    enabled: Boolean(userId),
  });

  const user = users.data?.find((candidate) => candidate.id === userId);
  const devices = useMemo(
    () => new Set(actions.data?.map((action) => action.device_id) ?? []).size,
    [actions.data],
  );

  function applyRange(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setRange(draftRange);
  }

  return (
    <section>
      <Link
        to="/"
        className="mb-7 inline-flex items-center gap-2 text-sm font-medium text-slate-500 transition-colors hover:text-ink"
      >
        <ArrowLeft className="size-4" aria-hidden="true" />
        All users
      </Link>

      <div className="mb-8 flex flex-col justify-between gap-6 lg:flex-row lg:items-end">
        <div>
          <p className="mb-3 text-xs font-semibold uppercase tracking-[0.18em] text-accent">
            User activity
          </p>
          <h1 className="text-3xl font-semibold tracking-[-0.04em] sm:text-4xl">
            {users.isPending ? "Loading user…" : user?.name ?? "Unknown user"}
          </h1>
          <p className="mt-3 font-mono text-xs text-slate-400">{userId}</p>
        </div>

        <form
          onSubmit={applyRange}
          className="flex flex-col gap-3 rounded-2xl border border-black/[0.06] bg-white p-3 shadow-sm sm:flex-row sm:items-end"
        >
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-slate-500">From</span>
            <input
              type="datetime-local"
              value={draftRange.start}
              max={draftRange.end}
              onChange={(event) =>
                setDraftRange((current) => ({ ...current, start: event.target.value }))
              }
              required
              className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm outline-none transition focus:border-accent focus:ring-2 focus:ring-accent/10"
            />
          </label>
          <label className="block">
            <span className="mb-1.5 block text-xs font-medium text-slate-500">To</span>
            <input
              type="datetime-local"
              value={draftRange.end}
              min={draftRange.start}
              onChange={(event) =>
                setDraftRange((current) => ({ ...current, end: event.target.value }))
              }
              required
              className="w-full rounded-xl border border-slate-200 bg-white px-3 py-2 text-sm outline-none transition focus:border-accent focus:ring-2 focus:ring-accent/10"
            />
          </label>
          <button
            type="submit"
            className="inline-flex items-center justify-center gap-2 rounded-xl bg-ink px-4 py-2.5 text-sm font-medium text-white transition hover:bg-slate-700 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent"
          >
            <CalendarRange className="size-4" aria-hidden="true" />
            Apply
          </button>
        </form>
      </div>

      <div className="mb-6 grid gap-4 sm:grid-cols-2">
        <div className="rounded-2xl border border-black/[0.06] bg-white p-5 shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm text-slate-500">Events in range</p>
              <p className="mt-2 text-3xl font-semibold tracking-tight">{actions.data?.length ?? "—"}</p>
            </div>
            <span className="grid size-11 place-items-center rounded-2xl bg-emerald-50 text-accent">
              <Radio className="size-5" aria-hidden="true" />
            </span>
          </div>
        </div>
        <div className="rounded-2xl border border-black/[0.06] bg-white p-5 shadow-sm">
          <div className="flex items-center justify-between">
            <div>
              <p className="text-sm text-slate-500">Devices represented</p>
              <p className="mt-2 text-3xl font-semibold tracking-tight">{actions.data ? devices : "—"}</p>
            </div>
            <span className="grid size-11 place-items-center rounded-2xl bg-slate-100 text-slate-600">
              <Laptop2 className="size-5" aria-hidden="true" />
            </span>
          </div>
        </div>
      </div>

      <div className="overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
        {actions.isPending ? (
          <LoadingState label="Loading activity…" />
        ) : actions.isError ? (
          <ErrorState error={actions.error} />
        ) : actions.data.length === 0 ? (
          <div className="grid min-h-56 place-items-center px-5 text-center text-sm text-slate-500">
            No activity was recorded in this time range.
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full min-w-[760px] border-collapse text-left">
              <thead>
                <tr className="border-b border-slate-100 bg-slate-50/70 text-xs uppercase tracking-wider text-slate-500">
                  <th className="px-6 py-4 font-medium">Time</th>
                  <th className="px-6 py-4 font-medium">Event</th>
                  <th className="px-6 py-4 font-medium">Details</th>
                  <th className="px-6 py-4 font-medium">Device</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {actions.data.map((action) => {
                  const kind = presentActionKind(action.kind);
                  return (
                    <tr key={action.id} className="align-top transition-colors hover:bg-slate-50/70">
                      <td className="whitespace-nowrap px-6 py-4 text-sm text-slate-600">
                        {formatDateTime(action.occurred_at)}
                      </td>
                      <td className="px-6 py-4">
                        <span
                          className={`inline-flex rounded-full px-2.5 py-1 text-xs font-medium ring-1 ring-inset ${toneClasses[kind.tone]}`}
                        >
                          {kind.label}
                        </span>
                      </td>
                      <td className="max-w-md px-6 py-4 text-sm text-slate-600">{kind.detail}</td>
                      <td className="px-6 py-4 font-mono text-xs text-slate-400">
                        {action.device_id}
                      </td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}
