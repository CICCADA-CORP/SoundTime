import type { SetupStatus } from "$lib/types";

export function isSetupStatus(value: unknown): value is SetupStatus {
  return !!value && typeof value === "object"
    && "setup_complete" in value && typeof value.setup_complete === "boolean"
    && "has_admin" in value && typeof value.has_admin === "boolean"
    && "instance_private" in value && typeof value.instance_private === "boolean";
}

export function localDestination(value: string | null, origin: string): string {
  if (!value?.startsWith("/") || value.startsWith("//") || value.includes("\\")) return "/";
  try {
    const url = new URL(value, origin);
    const path = url.pathname;
    if (url.origin !== origin || path.startsWith("//") || /^\/(login|register|api|_app)(\/|$)/.test(path)) return "/";
    return path + url.search + url.hash;
  } catch {
    return "/";
  }
}

export function accountLink(path: "/login" | "/register", destination: string) {
  return destination === "/" ? path : path + "?next=" + encodeURIComponent(destination);
}

type PageUrl = Pick<URL, "pathname" | "search" | "hash">;

export function signInLink(url: PageUrl) {
  return accountLink("/login", url.pathname + url.search + url.hash);
}

export function accessRedirect(status: SetupStatus, url: PageUrl, authenticated: boolean): string | null {
  const { pathname } = url;
  if (!status.setup_complete) {
    if (pathname === "/setup" || (status.has_admin && pathname === "/login")) return null;
    return "/setup";
  }
  if (pathname === "/setup") return "/";
  if (status.instance_private && !authenticated && pathname !== "/login" && pathname !== "/register") {
    return signInLink(url);
  }
  return null;
}
