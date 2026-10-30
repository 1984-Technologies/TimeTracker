import { ArrowLeft } from "lucide-react";
import { Link } from "react-router-dom";

export function NotFoundPage() {
  return (
    <div className="grid min-h-[55vh] place-items-center text-center">
      <div>
        <p className="text-sm font-semibold text-accent">404</p>
        <h1 className="mt-3 text-3xl font-semibold tracking-tight">Page not found</h1>
        <p className="mt-3 text-sm text-slate-500">The dashboard page you requested does not exist.</p>
        <Link
          to="/"
          className="mt-6 inline-flex items-center gap-2 rounded-xl bg-ink px-4 py-2.5 text-sm font-medium text-white"
        >
          <ArrowLeft className="size-4" aria-hidden="true" />
          Back to users
        </Link>
      </div>
    </div>
  );
}
