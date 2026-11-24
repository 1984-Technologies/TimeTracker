import { config } from "./config";
import { clearSession, getToken, sessionSignal, type TokenResponse } from "./session";
import type { UserAction, UserSummary } from "./types";

export interface AdminAccount { id: string; username: string }
export interface DeviceSummary { device_id: string; user_id: string }
export interface Enrollment { device_id: string; credential_hash: string }
export class ApiError extends Error {
  constructor(public status: number, message: string) { super(message); }
}

export async function request<T>(path: string, signal?: AbortSignal, method = "GET", body?: unknown, authenticated = true): Promise<T> {
  const token = authenticated ? getToken() : null;
  const session = sessionSignal();
  const headers: Record<string, string> = { Accept: "application/json" };
  if (token) headers.Authorization = `Bearer ${token}`;
  if (body !== undefined) headers["Content-Type"] = "application/json";
  const response = await fetch(`${config.apiBaseUrl}${path}`, {
    method, headers, signal: authenticated ? AbortSignal.any([session, ...(signal ? [signal] : [])]) : signal,
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) {
    if (response.status === 401 && token && token === getToken()) clearSession();
    const error = await response.json().catch(() => null) as { message?: string } | null;
    throw new ApiError(response.status, error?.message || `Request failed with status ${response.status}`);
  }
  if (authenticated) session.throwIfAborted();
  if (response.status === 204) return undefined as T;
  const value = await response.json() as T;
  if (authenticated) session.throwIfAborted();
  return value;
}
export const login = (username: string, password: string) => request<TokenResponse>("/api/auth/login", undefined, "POST", { username, password }, false);
export const currentAdmin = (signal?: AbortSignal) => request<AdminAccount>("/api/auth/me", signal);
export const getAdmins = (signal?: AbortSignal) => request<AdminAccount[]>("/api/admins", signal);
export const createAdmin = (username: string, password: string) => request<AdminAccount>("/api/admins", undefined, "POST", { username, password });
export const changePassword = (id: string, password: string) => request<void>(`/api/admins/${encodeURIComponent(id)}`, undefined, "PATCH", { password });
export const getDevices = (signal?: AbortSignal) => request<DeviceSummary[]>("/api/devices", signal);
export const approveDevice = (enrollment: Enrollment, user_id: string) => request<DeviceSummary>("/api/devices", undefined, "POST", { ...enrollment, user_id });
export const createUser = (name: string) => request<UserSummary>("/users", undefined, "POST", { name });
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
