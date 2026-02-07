import { api, clearTokens, setTokens, apiFetch } from "$lib/api";
import type { AuthResponse, User } from "$lib/types";

let user = $state<User | null>(null);
let loading = $state(true);

function isAuthenticated(): boolean {
  return user !== null;
}

function isAdmin(): boolean {
  return user?.role === "admin";
}

async function login(username: string, password: string): Promise<void> {
  const data = await api.post<AuthResponse>("/auth/login", {
    username,
    password,
  });
  setTokens(data.tokens.access_token, data.tokens.refresh_token);
  user = data.user;
}

async function register(
  email: string,
  username: string,
  password: string
): Promise<void> {
  const data = await api.post<AuthResponse>("/auth/register", {
    email,
    username,
    password,
  });
  setTokens(data.tokens.access_token, data.tokens.refresh_token);
  user = data.user;
}

function logout() {
  clearTokens();
  user = null;
}

async function deleteAccount(password: string): Promise<void> {
  await apiFetch("/auth/account", {
    method: "DELETE",
    body: JSON.stringify({ password }),
  });
  clearTokens();
  user = null;
}

async function updateEmail(newEmail: string, password: string): Promise<void> {
  const updated = await api.put<User>("/auth/email", { new_email: newEmail, password });
  user = updated;
}

async function updatePassword(currentPassword: string, newPassword: string): Promise<void> {
  await apiFetch("/auth/password", {
    method: "PUT",
    body: JSON.stringify({ current_password: currentPassword, new_password: newPassword }),
  });
}

async function fetchMe(): Promise<void> {
  try {
    user = await api.get<User>("/auth/me");
  } catch {
    user = null;
  }
}

async function init(): Promise<void> {
  loading = true;
  if (typeof window !== "undefined" && localStorage.getItem("soundtime_access_token")) {
    await fetchMe();
  }
  loading = false;
}

export function getAuthStore() {
  return {
    get user() { return user; },
    get loading() { return loading; },
    get isAuthenticated() { return isAuthenticated(); },
    get isAdmin() { return isAdmin(); },
    login,
    register,
    logout,
    deleteAccount,
    updateEmail,
    updatePassword,
    init,
    fetchMe,
  };
}
