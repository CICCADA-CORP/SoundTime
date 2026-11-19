import { describe, expect, it } from "vitest";
import { accessRedirect, accountLink, isSetupStatus, localDestination } from "./navigation";

const origin = "https://music.example";
const publicStatus = { setup_complete: true, has_admin: true, instance_private: false };
const privateStatus = { ...publicStatus, instance_private: true };
const unfinished = { ...publicStatus, setup_complete: false };
const album = new URL("/albums/42?sort=recent#title", origin);

describe("instance access", () => {
  it("recognizes only a complete status with explicit visibility", () => {
    for (const value of [null, undefined, "public", { ...publicStatus, instance_private: undefined }, { ...privateStatus, has_admin: "true" }]) {
      expect(isSetupStatus(value)).toBe(false);
    }
    expect(isSetupStatus(publicStatus)).toBe(true);
    expect(isSetupStatus(privateStatus)).toBe(true);
  });

  it("retains the complete destination of an anonymous private visit", () => {
    const login = accessRedirect(privateStatus, album, false);
    expect(login).toBe("/login?next=%2Falbums%2F42%3Fsort%3Drecent%23title");
    expect(localDestination(new URL(login!, origin).searchParams.get("next"), origin)).toBe("/albums/42?sort=recent#title");
    for (const path of ["/login", "/register"]) {
      expect(accessRedirect(privateStatus, new URL(path, origin), false)).toBeNull();
    }
    expect(accessRedirect(publicStatus, album, false)).toBeNull();
    expect(accessRedirect(privateStatus, album, true)).toBeNull();
    expect(accountLink("/login", "/")).toBe("/login");
  });

  it("resumes unfinished setup and leaves the login route available to the existing administrator", () => {
    const setup = new URL("/setup", origin);
    const login = new URL("/login", origin);
    expect(accessRedirect(unfinished, album, false)).toBe("/setup");
    expect(accessRedirect(unfinished, setup, true)).toBeNull();
    expect(accessRedirect(unfinished, login, false)).toBeNull();
    expect(accessRedirect({ ...unfinished, has_admin: false }, login, false)).toBe("/setup");
    expect(accessRedirect(publicStatus, setup, true)).toBe("/");
    expect(localDestination("/setup", origin)).toBe("/setup");
  });

  it.each([null, "javascript:bad", "//other.example", "/\\other.example", "/.//other.example", "/api/auth", "/abc/../login", "/register?next=/", "/_app/chunk"])(
    "uses the home route for an invalid or recursive destination %s", value => {
      expect(localDestination(value, origin)).toBe("/");
    }
  );
});
