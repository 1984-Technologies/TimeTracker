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

interface PaginatedResponse<T> {
  items: T[];
  total: number;
  limit: number;
  offset: number;
}

export function getUserActions(
  userId: string,
  query: ActionsQuery,
  signal?: AbortSignal,
): Promise<UserAction[]> {
  const params = new URLSearchParams({ start: query.start, end: query.end });
  return request<PaginatedResponse<UserAction>>(
    `/users/${encodeURIComponent(userId)}/actions?${params.toString()}`,
    signal,
  ).then((response) => response.items);
}
