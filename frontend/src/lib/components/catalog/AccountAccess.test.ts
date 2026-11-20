import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/svelte";
import { setLocale } from "$lib/i18n/index.svelte";
import Login from "../../../routes/login/+page.svelte";
import Register from "../../../routes/register/+page.svelte";

const auth = vi.hoisted(() => ({ login: vi.fn(), register: vi.fn() }));
const goto = vi.hoisted(() => vi.fn());
const page = vi.hoisted(() => ({ url: new URL("http://localhost/") }));
vi.mock("$app/state", () => ({ page }));
vi.mock("$lib/stores/auth.svelte", () => ({ getAuthStore: () => auth }));
vi.mock("$app/navigation", () => ({ goto }));

beforeEach(() => { vi.resetAllMocks(); setLocale("en"); page.url = new URL("http://localhost/"); });
afterEach(() => vi.unstubAllGlobals());

it("shows a login failure then navigates home with the corrected credentials", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json({ open_registration: true })));
  auth.login.mockRejectedValueOnce(new Error("Invalid credentials")).mockResolvedValueOnce(undefined);
  render(Login);
  expect(screen.getByRole("heading", { name: "Sign in to your account" })).toBeVisible();

  expect(await screen.findByRole("link", { name: "Sign up" })).toHaveAttribute("href", "/register");
  await fireEvent.input(screen.getByLabelText("Username"), { target: { value: "listener" } });
  const password = screen.getByLabelText("Password", { exact: true });
  await fireEvent.input(password, { target: { value: "incorrect" } });
  await fireEvent.click(screen.getByRole("button", { name: "Show password" }));
  expect(password).toHaveAttribute("type", "text");
  await fireEvent.click(screen.getByRole("button", { name: "Hide password" }));
  expect(password).toHaveAttribute("type", "password");
  await fireEvent.submit(password.closest("form")!);
  expect(await screen.findByRole("alert")).toHaveTextContent("Invalid credentials");
  expect(goto).not.toHaveBeenCalled();

  await fireEvent.input(password, { target: { value: "corrected-secret" } });
  await fireEvent.submit(password.closest("form")!);
  await waitFor(() => expect(goto).toHaveBeenCalledWith("/"));
  expect(auth.login).toHaveBeenLastCalledWith("listener", "corrected-secret");
});

it("validates password confirmation before sending the registration", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json({ open_registration: true })));
  render(Register);
  const email = await screen.findByLabelText("Email");
  await fireEvent.input(email, { target: { value: "listener@example.test" } });
  await fireEvent.input(screen.getByLabelText("Username"), { target: { value: "listener" } });
  await fireEvent.input(screen.getByLabelText("Password", { exact: true }), { target: { value: "welcome-home" } });
  const confirmation = screen.getByLabelText("Confirm Password");
  await fireEvent.input(confirmation, { target: { value: "another-value" } });
  await fireEvent.submit(email.closest("form")!);
  expect(await screen.findByRole("alert")).toHaveTextContent("Passwords do not match");
  expect(auth.register).not.toHaveBeenCalled();

  await fireEvent.input(confirmation, { target: { value: "welcome-home" } });
  await fireEvent.submit(email.closest("form")!);
  await waitFor(() => expect(goto).toHaveBeenCalledWith("/"));
  expect(auth.register).toHaveBeenCalledWith("listener@example.test", "listener", "welcome-home");
});

it("directs users of a closed instance to sign in", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json({ open_registration: false })));
  render(Register);
  expect(await screen.findByRole("heading", { name: "Registration Closed" })).toBeVisible();
  expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login");
  expect(screen.queryByLabelText("Username")).toBeNull();
});

it("keeps the sign-in form usable without offering registration on a private instance", async () => {
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json({ open_registration: false })));
  let finish!: () => void;
  auth.login.mockImplementation(() => new Promise<void>(resolve => { finish = resolve; }));
  render(Login);
  const password = screen.getByLabelText("Password", { exact: true });
  await fireEvent.input(screen.getByLabelText("Username"), { target: { value: "listener" } });
  await fireEvent.input(password, { target: { value: "welcome-home" } });
  const form = password.closest("form")!;
  await fireEvent.submit(form);
  expect(form).toHaveAttribute("aria-busy", "true");
  expect(form.querySelector('button[type="submit"]')).toBeDisabled();
  await fireEvent.submit(form);
  expect(auth.login).toHaveBeenCalledTimes(1);
  expect(screen.queryByRole("link", { name: "Sign up" })).toBeNull();
  finish();
  await waitFor(() => expect(goto).toHaveBeenCalledWith("/"));
});

it("returns the listener to their private track after signing in", async () => {
  const next = "/tracks/one?mode=full#lyrics";
  page.url = new URL("/login?next=" + encodeURIComponent(next), "http://localhost");
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json({ open_registration: false })));
  render(Login);
  const password = screen.getByLabelText("Password", { exact: true });
  await fireEvent.input(screen.getByLabelText("Username"), { target: { value: "listener" } });
  await fireEvent.input(password, { target: { value: "corrected-secret" } });
  await fireEvent.submit(password.closest("form")!);
  await waitFor(() => expect(goto).toHaveBeenCalledWith(next));
});

it("waits for a confirmed registration setting and allows retry after an outage", async () => {
  page.url = new URL("/register?next=%2Falbums%2F2", "http://localhost");
  const fetch = vi.fn().mockRejectedValueOnce(new TypeError("offline"))
    .mockResolvedValueOnce(Response.json({ open_registration: true }));
  vi.stubGlobal("fetch", fetch);
  render(Register);
  expect(await screen.findByText("Registration settings could not be loaded.")).toBeVisible();
  expect(screen.queryByLabelText("Username")).toBeNull();
  await fireEvent.click(screen.getByRole("button", { name: "Retry connection" }));
  expect(await screen.findByLabelText("Username")).toBeVisible();
  expect(screen.getByRole("link", { name: "Sign in" })).toHaveAttribute("href", "/login?next=%2Falbums%2F2");
});
