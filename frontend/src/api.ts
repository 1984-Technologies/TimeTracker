import { config } from "./config";
import type { UserAction, UserSummary } from "./types";

async function request<T>(path: string, signal?: AbortSignal): Promise<T> {
  const response = await fetch(`${config.apiBaseUrl}${path}`, {
    headers: { Accept: "application/json" },
    signal,
  });

  if (!response.ok) {
    const detail = await response.text();
    throw new Error(detail || `Request failed with status ${response.status}`);
  }

  return response.json() as Promise<T>;
}

export function getUsers(signal?: AbortSignal): Promise<UserSummary[]> {
  return request<UserSummary[]>("/users", signal);
}

export interface ActionsQuery {
  start: string;
  end: string;
}

interface ActionsPage {
  items: UserAction[];
  total: number;
}

export async function getUserActions(
  userId: string,
  query: ActionsQuery,
  signal?: AbortSignal,
): Promise<UserAction[]> {
  const actions: UserAction[] = [];
  const params = new URLSearchParams({ start: query.start, end: query.end, limit: "1000" });
  while (true) {
    params.set("offset", String(actions.length));
    const page = await request<ActionsPage | UserAction[]>(
      `/users/${encodeURIComponent(userId)}/actions?${params.toString()}`,
      signal,
    );
    // The mock API returns an unpaginated array.
    if (Array.isArray(page)) return page;
    actions.push(...page.items);
    if (page.items.length === 0 || actions.length >= page.total) return actions;
  }
}
