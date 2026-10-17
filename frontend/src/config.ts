const configuredApiUrl = import.meta.env.VITE_API_BASE_URL?.trim();

export const config = {
  apiBaseUrl: (configuredApiUrl || "http://localhost:8239").replace(/\/$/, ""),
} as const;
