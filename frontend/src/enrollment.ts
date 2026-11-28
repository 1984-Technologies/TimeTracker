import type { Enrollment } from "./api";

export function parseEnrollment(value: string, expected: string): Enrollment {
  const url = new URL(value);
  const target = new URL(expected);
  const params = new URLSearchParams(url.hash.slice(1));
  const device_id = params.get("device_id") ?? "";
  const credential_hash = params.get("credential_hash") ?? "";
  if (!/^https?:$/.test(url.protocol) || url.origin !== target.origin || url.pathname !== target.pathname || url.search || url.username || url.password || params.size !== 2 || params.getAll("device_id").length !== 1 || params.getAll("credential_hash").length !== 1 || !/^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(device_id) || !/^[0-9a-f]{64}$/.test(credential_hash)) throw new Error("Use a registration link from this dashboard with a valid device ID and credential hash.");
  return { device_id: device_id.toLowerCase(), credential_hash };
}
