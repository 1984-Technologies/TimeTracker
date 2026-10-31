import { useQuery } from "@tanstack/react-query";
import { ArrowRight, UsersRound } from "lucide-react";
import { Link } from "react-router-dom";
import { getUsers } from "../api";
import { ErrorState, LoadingState } from "../components/Feedback";

export function UsersPage() {
  const users = useQuery({
    queryKey: ["users"],
    queryFn: ({ signal }) => getUsers(signal),
  });

  return (
    <section>
      <div className="mb-8 flex flex-col justify-between gap-5 sm:flex-row sm:items-end">
        <div>
          <p className="mb-3 text-xs font-semibold uppercase tracking-[0.18em] text-accent">
            Overview
          </p>
          <h1 className="text-3xl font-semibold tracking-[-0.04em] sm:text-4xl">Team activity</h1>
          <p className="mt-3 max-w-xl text-sm leading-6 text-slate-500 sm:text-base">
            Select a user to inspect their activity events for any time range.
          </p>
        </div>
        <div className="flex w-fit items-center gap-3 rounded-2xl border border-black/5 bg-white px-4 py-3 shadow-sm">
          <span className="grid size-9 place-items-center rounded-xl bg-emerald-50 text-accent">
            <UsersRound className="size-4" aria-hidden="true" />
          </span>
          <div>
            <p className="text-xs text-slate-500">Tracked users</p>
            <p className="text-lg font-semibold leading-5">{users.data?.length ?? "—"}</p>
          </div>
        </div>
      </div>

      <div className="overflow-hidden rounded-2xl border border-black/[0.06] bg-white shadow-panel">
        {users.isPending ? (
          <LoadingState label="Loading users…" />
        ) : users.isError ? (
          <ErrorState error={users.error} />
        ) : users.data.length === 0 ? (
          <div className="grid min-h-56 place-items-center px-5 text-sm text-slate-500">
            No users are available yet.
          </div>
        ) : (
          <div className="overflow-x-auto">
            <table className="w-full border-collapse text-left">
              <thead>
                <tr className="border-b border-slate-100 bg-slate-50/70 text-xs uppercase tracking-wider text-slate-500">
                  <th className="px-5 py-4 font-medium sm:px-6">User</th>
                  <th className="hidden px-5 py-4 font-medium md:table-cell">Identifier</th>
                  <th className="w-16 px-5 py-4 sm:px-6">
                    <span className="sr-only">Open</span>
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-slate-100">
                {users.data.map((user) => (
                  <tr key={user.id} className="group transition-colors hover:bg-emerald-50/40">
                    <td className="p-0">
                      <Link
                        to={`/users/${user.id}`}
                        className="flex items-center gap-3 px-5 py-5 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent sm:px-6"
                      >
                        <span className="grid size-9 shrink-0 place-items-center rounded-full bg-ink text-sm font-semibold text-white">
                          {user.name.trim().charAt(0).toUpperCase() || "?"}
                        </span>
                        <span className="font-medium">{user.name}</span>
                      </Link>
                    </td>
                    <td className="hidden p-0 md:table-cell">
                      <Link
                        to={`/users/${user.id}`}
                        className="block px-5 py-5 font-mono text-xs text-slate-400 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent"
                      >
                        {user.id}
                      </Link>
                    </td>
                    <td className="p-0">
                      <Link
                        to={`/users/${user.id}`}
                        className="grid place-items-center px-5 py-5 text-slate-400 transition-colors group-hover:text-accent focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-[-2px] focus-visible:outline-accent sm:px-6"
                        aria-label={`View ${user.name}`}
                      >
                        <ArrowRight className="size-4" aria-hidden="true" />
                      </Link>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
    </section>
  );
}
