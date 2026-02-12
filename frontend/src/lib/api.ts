import type { ApiError, TokenPair } from "./types";

/**
 * API base URL.
 * - Production (behind Nginx): relative "/api" — works for any domain/IP.
 * - Dev override: set PUBLIC_API_URL in .env (e.g. http://localhost:8080/api)
 *   when NOT using the Vite proxy.
 */
const API_BASE: string = import.meta.env.PUBLIC_API_URL ?? "/api";

function getAccessToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("soundtime_access_token");
}

function getRefreshToken(): string | null {
  if (typeof window === "undefined") return null;
  return localStorage.getItem("soundtime_refresh_token");
}

function setTokens(access: string, refresh: string) {
  localStorage.setItem("soundtime_access_token", access);
  localStorage.setItem("soundtime_refresh_token", refresh);
}

export function clearTokens() {
  localStorage.removeItem("soundtime_access_token");
  localStorage.removeItem("soundtime_refresh_token");
}

async function refreshAccessToken(): Promise<boolean> {
  const refreshToken = getRefreshToken();
  if (!refreshToken) return false;

  try {
    const res = await fetch(`${API_BASE}/auth/refresh`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ refresh_token: refreshToken }),
    });

    if (!res.ok) {
      clearTokens();
      return false;
    }

    const data: TokenPair = await res.json();
    setTokens(data.access_token, data.refresh_token);
    return true;
  } catch {
    clearTokens();
    return false;
  }
}

export async function apiFetch<T = unknown>(
  path: string,
  options: RequestInit = {}
): Promise<T> {
  const headers = new Headers(options.headers ?? {});
  const token = getAccessToken();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }

  if (
    !headers.has("Content-Type") &&
    !(options.body instanceof FormData)
  ) {
    headers.set("Content-Type", "application/json");
  }

  let res = await fetch(`${API_BASE}${path}`, { ...options, headers });

  // Auto-refresh on 401
  if (res.status === 401 && token) {
    const refreshed = await refreshAccessToken();
    if (refreshed) {
      const newToken = getAccessToken();
      if (newToken) {
        headers.set("Authorization", `Bearer ${newToken}`);
      }
      res = await fetch(`${API_BASE}${path}`, { ...options, headers });
    }
  }

  if (!res.ok) {
    const err: ApiError = await res.json().catch(() => ({
      error: `HTTP ${res.status}`,
    }));
    throw new Error(err.error);
  }

  if (res.status === 204) return undefined as T;
  const text = await res.text();
  if (!text) return undefined as T;
  return JSON.parse(text);
}

export const api = {
  get: <T = unknown>(path: string) => apiFetch<T>(path),

  post: <T = unknown>(path: string, body?: unknown) =>
    apiFetch<T>(path, {
      method: "POST",
      body: body ? JSON.stringify(body) : undefined,
    }),

  put: <T = unknown>(path: string, body?: unknown) =>
    apiFetch<T>(path, {
      method: "PUT",
      body: body ? JSON.stringify(body) : undefined,
    }),

  patch: <T = unknown>(path: string, body?: unknown) =>
    apiFetch<T>(path, {
      method: "PATCH",
      body: body ? JSON.stringify(body) : undefined,
    }),

  delete: <T = unknown>(path: string) =>
    apiFetch<T>(path, { method: "DELETE" }),

  upload: <T = unknown>(path: string, formData: FormData) =>
    apiFetch<T>(path, {
      method: "POST",
      body: formData,
    }),

  /**
   * Upload with progress tracking via XMLHttpRequest.
   * Returns a promise + abort controller.
   */
  uploadWithProgress: <T = unknown>(
    path: string,
    formData: FormData,
    onProgress?: (loaded: number, total: number) => void,
  ): { promise: Promise<T>; abort: () => void } => {
    const xhr = new XMLHttpRequest();
    const promise = new Promise<T>((resolve, reject) => {
      xhr.open("POST", `${API_BASE}${path}`);

      const token = getAccessToken();
      if (token) {
        xhr.setRequestHeader("Authorization", `Bearer ${token}`);
      }

      xhr.upload.addEventListener("progress", (e) => {
        if (e.lengthComputable && onProgress) {
          onProgress(e.loaded, e.total);
        }
      });

      xhr.addEventListener("load", () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          try {
            resolve(JSON.parse(xhr.responseText));
          } catch {
            resolve(undefined as T);
          }
        } else {
          try {
            const err = JSON.parse(xhr.responseText);
            reject(new Error(err.error || `HTTP ${xhr.status}`));
          } catch {
            reject(new Error(`HTTP ${xhr.status}`));
          }
        }
      });

      xhr.addEventListener("error", () => reject(new Error("Network error")));
      xhr.addEventListener("abort", () => reject(new Error("Upload cancelled")));

      xhr.send(formData);
    });

    return { promise, abort: () => xhr.abort() };
  },
};

export function streamUrl(trackId: string): string {
  const token = getAccessToken();
  return `${API_BASE}/tracks/${trackId}/stream${token ? `?token=${token}` : ""}`;
}

// ─── Plugin API ─────────────────────────────────────────────────────

export const pluginApi = {
  /** List all installed plugins. */
  list: () => api.get<import("./types").PluginListResponse>("/admin/plugins"),

  /** Install a plugin from a git repository URL. */
  install: (gitUrl: string) =>
    api.post<import("./types").Plugin>("/admin/plugins/install", { git_url: gitUrl }),

  /** Enable a plugin. */
  enable: (id: string) => api.post<{ status: string }>(`/admin/plugins/${id}/enable`),

  /** Disable a plugin. */
  disable: (id: string) => api.post<{ status: string }>(`/admin/plugins/${id}/disable`),

  /** Uninstall a plugin. */
  uninstall: (id: string) => api.delete(`/admin/plugins/${id}`),

  /** Update a plugin from its git repository. */
  update: (id: string) => api.post<import("./types").Plugin>(`/admin/plugins/${id}/update`),

  /** Get plugin configuration. */
  getConfig: (id: string) =>
    api.get<import("./types").PluginConfigResponse>(`/admin/plugins/${id}/config`),

  /** Update plugin configuration. */
  updateConfig: (id: string, config: import("./types").PluginConfig[]) =>
    api.put<{ updated: number }>(`/admin/plugins/${id}/config`, { config }),

  /** Get plugin event logs. */
  getLogs: (id: string, page = 1, perPage = 50) =>
    api.get<import("./types").PluginLogsResponse>(
      `/admin/plugins/${id}/logs?page=${page}&per_page=${perPage}`
    ),
};

// ─── Theme API ──────────────────────────────────────────────────────

export const themeApi = {
  /** List all installed themes. */
  list: () => api.get<import("./types").ThemeListResponse>("/admin/themes"),

  /** Install a theme from a git URL. */
  install: (gitUrl: string) =>
    api.post<import("./types").Theme>("/admin/themes/install", { git_url: gitUrl }),

  /** Enable (activate) a theme. */
  enable: (id: string) => api.post<import("./types").Theme>(`/admin/themes/${id}/enable`),

  /** Disable a theme. */
  disable: (id: string) => api.post<import("./types").Theme>(`/admin/themes/${id}/disable`),

  /** Update a theme from its git repository. */
  update: (id: string) => api.post<import("./types").Theme>(`/admin/themes/${id}/update`),

  /** Uninstall a theme. */
  uninstall: (id: string) => api.delete(`/admin/themes/${id}`),

  /** Get the currently active theme (or null if none). */
  active: () => api.get<import("./types").Theme>("/themes/active"),
};

export { setTokens, API_BASE };
