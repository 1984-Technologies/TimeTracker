import { AlertCircle, LoaderCircle } from "lucide-react";

export function LoadingState({ label }: { label: string }) {
  return (
    <div className="flex min-h-56 items-center justify-center gap-3 text-sm text-slate-500">
      <LoaderCircle className="size-5 animate-spin" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}

export function ErrorState({ error }: { error: Error }) {
  return (
    <div className="flex min-h-56 flex-col items-center justify-center gap-3 px-6 text-center">
      <span className="grid size-10 place-items-center rounded-full bg-red-50 text-red-600">
        <AlertCircle className="size-5" aria-hidden="true" />
      </span>
      <div>
        <p className="font-medium text-slate-900">Could not load this data</p>
        <p className="mt-1 max-w-lg text-sm text-slate-500">{error.message}</p>
      </div>
    </div>
  );
}
