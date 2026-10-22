import type { AdapterStatus, UserActionKind } from "./types";

export interface KindPresentation {
  label: string;
  detail: string;
  tone: "green" | "amber" | "slate" | "red";
}

function adapterStatusLabel(status: AdapterStatus): string {
  if (status === "Available") return "Available";
  return `Unavailable · ${status.Unavailable.reason}`;
}

export function presentActionKind(kind: UserActionKind): KindPresentation {
  if (kind === "CollectorStarted") {
    return { label: "Collector started", detail: "Tracking resumed", tone: "green" };
  }
  if (kind === "CollectorStopped") {
    return { label: "Collector stopped", detail: "Tracking paused", tone: "red" };
  }
  if (kind === "Heartbeat") {
    return { label: "Heartbeat", detail: "Collector is healthy", tone: "slate" };
  }
  if ("IdleChanged" in kind) {
    return {
      label: kind.IdleChanged.idle ? "Idle" : "Active",
      detail: kind.IdleChanged.idle ? "User became idle" : "Activity resumed",
      tone: kind.IdleChanged.idle ? "amber" : "green",
    };
  }
  if ("FocusedApplicationChanged" in kind) {
    const focus = kind.FocusedApplicationChanged;
    return {
      label: "Application changed",
      detail: focus.title ? `${focus.app_id} · ${focus.title}` : focus.app_id,
      tone: "slate",
    };
  }

  const adapter = kind.AdapterStatus;
  return {
    label: "Adapter status",
    detail: `${adapter.adapter_id} · ${adapter.observation_type} · ${adapterStatusLabel(adapter.status)}`,
    tone: adapter.status === "Available" ? "green" : "red",
  };
}

export function formatDateTime(value: string): string {
  return new Intl.DateTimeFormat(undefined, {
    dateStyle: "medium",
    timeStyle: "medium",
  }).format(new Date(value));
}

export function toDateTimeLocal(date: Date): string {
  const offset = date.getTimezoneOffset();
  return new Date(date.getTime() - offset * 60_000).toISOString().slice(0, 16);
}
