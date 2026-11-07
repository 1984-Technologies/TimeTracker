import { useQuery } from "@tanstack/react-query";
import {
  ArrowLeft,
  CalendarRange,
  ChevronDown,
  Filter,
  Laptop2,
  Radio,
  type LucideIcon,
} from "lucide-react";
import { lazy, Suspense, useMemo, useState, type FormEvent } from "react";
import { Link, useParams } from "react-router-dom";
import { getUserActions, getUsers } from "../api";
import {
  actionKindName,
  eventKindOptions,
  sortActionsNewestFirst,
  type EventKindName,
} from "../analytics";
import { ErrorState, LoadingState } from "../components/Feedback";
import { SensorStatusGrid } from "../components/SensorStatusGrid";
import { formatDateTime, presentActionKind, toDateTimeLocal } from "../lib";

const AppActivityChart = lazy(() =>
  import("../components/AppActivityChart").then((module) => ({
    default: module.AppActivityChart,
  })),
);

const toneClasses = {
  green: "bg-emerald-50 text-emerald-700 ring-emerald-600/10",
  amber: "bg-amber-50 text-amber-700 ring-amber-600/10",
  slate: "bg-slate-100 text-slate-600 ring-slate-500/10",
  red: "bg-red-50 text-red-700 ring-red-600/10",
} as const;

const allEventKinds = eventKindOptions.map((option) => option.value);

function CompactMetric({
  icon: Icon,
  label,
  value,
  accent,
}: {
  icon: LucideIcon;
  label: string;
  value: number | string;
  accent: "green" | "slate";
}) {
  return (
    <div className="flex min-w-40 items-center gap-3 rounded-xl border border-black/[0.06] bg-white px-3 py-2.5 shadow-sm">
      <span
        className={`grid size-8 shrink-0 place-items-center rounded-lg ${
          accent === "green" ? "bg-emerald-50 text-accent" : "bg-slate-100 text-slate-600"
        }`}
      >
        <Icon className="size-4" aria-hidden="true" />
      </span>
      <div>
        <p className="text-[10px] font-medium uppercase tracking-wider text-slate-400">{label}</p>
        <p className="text-lg font-semibold leading-5 text-ink">{value}</p>
      </div>
    </div>
  );
}

function initialRange() {
  const end = new Date();
  const start = new Date(end.getTime() - 24 * 60 * 60 * 1000);
  return { start: toDateTimeLocal(start), end: toDateTimeLocal(end) };
}

export function UserPage() {
  const { userId = "" } = useParams();
  const [draftRange, setDraftRange] = useState(initialRange);
  const [range, setRange] = useState(initialRange);
  const [selectedEventKinds, setSelectedEventKinds] = useState<Set<EventKindName>>(
    () => new Set(allEventKinds),
  );
  const appliedRange = useMemo(
    () => ({
      start: new Date(range.start).toISOString(),
      end: new Date(range.end).toISOString(),
    }),
    [range],
  );

  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => getUsers(signal),
  });
  const actions = useQuery({
    queryKey: ["actions", userId, appliedRange.start, appliedRange.end],
    queryFn: ({ signal }) => getUserActions(userId, appliedRange, signal),
    enabled: Boolean(userId),
  });

  const user = users.data?.find((candidate) => candidate.id === userId);
  const devices = useMemo(
    () => new Set(actions.data?.map((action) => action.device_id) ?? []).size,
    [actions.data],
  );
  const orderedActions = useMemo(
    () => sortActionsNewestFirst(actions.data ?? []),
    [actions.data],
  );
  const filteredActions = useMemo(
    () => orderedActions.filter((action) => selectedEventKinds.has(actionKindName(action.kind))),
    [orderedActions, selectedEventKinds],
  );
  const eventFilterLabel =
    selectedEventKinds.size === allEventKinds.length
      ? "All event types"
      : selectedEventKinds.size === 0
        ? "No event types"
        : `${selectedEventKinds.size} of ${allEventKinds.length} event types`;

  function applyRange(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setRange(draftRange);
  }

  function toggleEventKind(kind: EventKindName) {
    setSelectedEventKinds((current) => {
      const next = new Set(current);
      if (next.has(kind)) next.delete(kind);
      else next.add(kind);
      return next;
    });
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

      <div className="mb-7 flex flex-col justify-between gap-5 xl:flex-row xl:items-end">
        <div className="flex flex-col gap-5 md:flex-row md:items-end">
          <div>
            <p className="mb-2 text-xs font-semibold uppercase tracking-[0.18em] text-accent">
              User activity
            </p>
            <h1 className="text-3xl font-semibold tracking-[-0.04em] sm:text-4xl">
              {users.isPending ? "Loading user…" : user?.name ?? "Unknown user"}
            </h1>
            <p className="mt-2 font-mono text-xs text-slate-400">{userId}</p>
          </div>
          <div className="flex flex-wrap gap-2">
            <CompactMetric
              icon={Radio}
              label="Events in range"
              value={actions.data?.length ?? "—"}
              accent="green"
            />
            <CompactMetric
              icon={Laptop2}
              label="Devices represented"
              value={actions.data ? devices : "—"}
              accent="slate"
            />
          </div>
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

      {actions.isPending ? (
        <div className="overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
          <LoadingState label="Loading activity…" />
        </div>
      ) : actions.isError ? (
        <div className="overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
          <ErrorState error={actions.error} />
        </div>
      ) : (
        <>
          <SensorStatusGrid actions={orderedActions} />
          <Suspense
            fallback={
              <div className="mb-6 overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
                <LoadingState label="Preparing application chart…" />
              </div>
            }
          >
            <AppActivityChart
              actions={orderedActions}
              rangeStart={appliedRange.start}
              rangeEnd={appliedRange.end}
            />
          </Suspense>

          <section className="relative z-10 rounded-2xl border border-black/[0.06] bg-white shadow-panel">
            <div className="flex flex-col gap-4 border-b border-slate-100 px-5 py-5 sm:flex-row sm:items-center sm:justify-between sm:px-6">
              <div>
                <h2 className="font-semibold tracking-tight">Event log</h2>
                <p className="mt-1 text-sm text-slate-500">
                  Newest first · showing {filteredActions.length} of {orderedActions.length} events
                </p>
              </div>
              <details className="group/filter relative sm:min-w-60">
                <summary className="flex cursor-pointer list-none items-center justify-between gap-3 rounded-xl border border-slate-200 bg-white px-3 py-2.5 text-sm font-medium text-slate-700 outline-none transition hover:border-slate-300 focus-visible:border-accent focus-visible:ring-2 focus-visible:ring-accent/10 [&::-webkit-details-marker]:hidden">
                  <span className="flex items-center gap-2">
                    <Filter className="size-3.5 text-slate-400" aria-hidden="true" />
                    {eventFilterLabel}
                  </span>
                  <ChevronDown
                    className="size-4 text-slate-400 transition-transform group-open/filter:rotate-180"
                    aria-hidden="true"
                  />
                </summary>
                <div className="absolute right-0 z-30 mt-2 w-72 rounded-2xl border border-slate-200 bg-white p-2 shadow-xl shadow-slate-900/10">
                  <div className="flex items-center justify-between border-b border-slate-100 px-2 pb-2 pt-1">
                    <span className="text-xs font-semibold text-slate-700">Event types</span>
                    <div className="flex gap-1">
                      <button
                        type="button"
                        onClick={() => setSelectedEventKinds(new Set(allEventKinds))}
                        className="rounded-lg px-2 py-1 text-xs font-medium text-accent transition hover:bg-emerald-50"
                      >
                        All
                      </button>
                      <button
                        type="button"
                        onClick={() => setSelectedEventKinds(new Set())}
                        className="rounded-lg px-2 py-1 text-xs font-medium text-slate-500 transition hover:bg-slate-100"
                      >
                        Clear
                      </button>
                    </div>
                  </div>
                  <div className="mt-1 space-y-0.5">
                    {eventKindOptions.map((option) => (
                      <label
                        key={option.value}
                        className="flex cursor-pointer items-center gap-3 rounded-xl px-2 py-2 text-sm text-slate-600 transition hover:bg-slate-50"
                      >
                        <input
                          type="checkbox"
                          checked={selectedEventKinds.has(option.value)}
                          onChange={() => toggleEventKind(option.value)}
                          className="size-4 cursor-pointer rounded border-slate-300 accent-accent"
                        />
                        <span>{option.label}</span>
                      </label>
                    ))}
                  </div>
                </div>
              </details>
            </div>

            {filteredActions.length === 0 ? (
              <div className="grid min-h-48 place-items-center rounded-b-2xl px-5 text-center text-sm text-slate-500">
                {orderedActions.length === 0
                  ? "No activity was recorded in this time range."
                  : "No events match this event-kind filter."}
              </div>
            ) : (
              <div className="overflow-x-auto rounded-b-2xl">
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
                    {filteredActions.map((action) => {
                      const kind = presentActionKind(action.kind);
                      return (
                        <tr
                          key={action.id}
                          className="align-top transition-colors hover:bg-slate-50/70"
                        >
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
                          <td className="max-w-md px-6 py-4 text-sm text-slate-600">
                            {kind.detail}
                          </td>
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
          </section>
        </>
      )}
    </section>
  );
}
