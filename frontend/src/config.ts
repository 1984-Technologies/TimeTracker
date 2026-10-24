const configuredApiUrl = import.meta.env.VITE_API_BASE_URL?.trim();

export const config = {
  apiBaseUrl: (configuredApiUrl || "").replace(/\/$/, ""),
} as const;
