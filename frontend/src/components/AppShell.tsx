import { Clock3 } from "lucide-react";
import { Link, Outlet } from "react-router-dom";

export function AppShell() {
  return (
    <div className="min-h-screen bg-canvas text-ink">
      <header className="border-b border-black/5 bg-white/70 backdrop-blur">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-5 py-4 sm:px-8">
          <Link to="/" className="flex items-center gap-3" aria-label="TimeTracker home">
            <span className="grid size-9 place-items-center rounded-xl bg-ink text-white shadow-sm">
              <Clock3 className="size-4" aria-hidden="true" />
            </span>
            <span>
              <span className="block text-sm font-semibold tracking-tight">TimeTracker</span>
              <span className="block text-xs text-slate-500">Activity dashboard</span>
            </span>
          </Link>
          <span className="rounded-full border border-emerald-200 bg-emerald-50 px-3 py-1 text-xs font-medium text-emerald-700">
            Dashboard
          </span>
        </div>
      </header>
      <main className="mx-auto max-w-7xl px-5 py-10 sm:px-8 sm:py-14">
        <Outlet />
      </main>
    </div>
  );
}
