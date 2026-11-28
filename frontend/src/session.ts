export interface TokenResponse { access_token: string; token_type: string; expires_in: number }
let token: string | null = null;
let timer: ReturnType<typeof setTimeout> | undefined;
let requests = new AbortController();
const listeners = new Set<() => void>();

export const getToken = () => token;
export const sessionSignal = () => requests.signal;
export function subscribeSession(listener: () => void) {
  listeners.add(listener);
  return () => { listeners.delete(listener); };
}
export function clearSession() {
  token = null;
  clearTimeout(timer);
  requests.abort();
  requests = new AbortController();
  for (const listener of listeners) listener();
}
export function setSession(response: TokenResponse) {
  if (response.token_type !== "Bearer" || !response.access_token || !Number.isFinite(response.expires_in) || response.expires_in <= 0 || response.expires_in > 900) throw new Error("Invalid login response");
  clearSession();
  token = response.access_token;
  timer = setTimeout(clearSession, response.expires_in * 1000);
  for (const listener of listeners) listener();
}
