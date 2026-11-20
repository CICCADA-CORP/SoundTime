"""Complete SoundTime's first-run API through loopback or a local SSH tunnel."""

import base64
import http.client
from http.cookies import SimpleCookie
import json

if __package__:
    from .cli_strings import text
else:
    from cli_strings import text


def _request(port, path, payload=None, headers=None):
    conn = http.client.HTTPConnection("127.0.0.1", port, timeout=12)
    try:
        body = json.dumps(payload).encode() if payload is not None else None
        conn.request("POST" if body is not None else "GET", path, body,
                     {"Content-Type": "application/json", **(headers or {})})
        response = conn.getresponse()
        cookies = response.headers.get_all("Set-Cookie") or []
        # Never follow redirects with credentials or echo raw API error bodies.
        raw = response.read(65536)
        if "application/json" in response.getheader("Content-Type", "") and 200 <= response.status < 300:
            obj = json.loads(raw)
        else:
            obj = {}
        return response.status, obj, cookies
    finally:
        conn.close()


def configure(port, profile, lang="en", *, bootstrap_password=None):
    """The CLI can defer credential prompts until after the initial status check."""
    if type(port) is not int or not 1 <= port <= 65535:
        raise ValueError(text("number"))
    if not isinstance(profile, dict) and not callable(profile):
        raise ValueError(text("profile"))
    if bootstrap_password is None and isinstance(profile, dict):
        bootstrap_password = profile.get("bootstrapPassword")
    headers = {}

    def request(path, payload=None):
        try:
            return _request(port, path, payload, headers)
        except (OSError, http.client.HTTPException, ValueError):
            raise RuntimeError(text("service")) from None

    def status():
        code, state, _ = request("/api/setup/status")
        if code == 401:
            password = bootstrap_password
            if not isinstance(password, str) or not password:
                raise ValueError(text("guard"))
            authorization = base64.b64encode(("soundtime:" + password).encode()).decode()
            headers["Authorization"] = "Basic " + authorization
            response, _, cookies = request("/setup")
            del headers["Authorization"]
            jar = SimpleCookie()
            for cookie in cookies:
                jar.load(cookie)
            if response != 200 or "soundtime_bootstrap" not in jar:
                raise ValueError(text("guard"))
            headers["Cookie"] = "soundtime_bootstrap=" + jar["soundtime_bootstrap"].value
            code, state, _ = request("/api/setup/status")
        if (code != 200 or not isinstance(state, dict) or type(state.get("has_admin")) is not bool
                or type(state.get("setup_complete")) is not bool):
            raise RuntimeError(text("service"))
        return state

    before = status()
    if before["setup_complete"]:
        return {"setupComplete": True, "alreadyComplete": True}
    if callable(profile):
        profile = profile()
    user, email, password, name = (profile.get(k) for k in ("username", "email", "password", "instanceName"))
    if (not all(isinstance(s, str) for s in (user, email, password, name))
            or not 3 <= len(user.encode()) <= 64 or not user.strip() or len(password) < 8
            or "@" not in email or not name.strip()):
        raise ValueError(text("profile"))
    private = profile.get("private", True)
    registrations = profile.get("openRegistrations", False)
    peer = profile.get("p2p", False)
    limit = profile.get("maxUploadMb", 500)
    description = profile.get("description", "")
    if (any(type(v) is not bool for v in (private, registrations, peer)) or (private and registrations)
            or type(limit) is not int or not 1 <= limit <= 4294967295 or not isinstance(description, str)):
        raise ValueError(text("bad_prefs"))

    login = {"username": user, "password": password}
    if not before["has_admin"]:
        code, response, _ = request("/api/setup/admin", {**login, "email": email})
        if code in (403, 409) and status()["has_admin"]:
            before["has_admin"] = True
    if before["has_admin"]:
        code, response, _ = request("/api/auth/login", login)
        if code not in (200, 201):
            raise ValueError(text("auth"))
    if code not in (200, 201) or not isinstance(response, dict):
        raise RuntimeError(text("setup_error"))
    tokens = response.get("tokens")
    if not isinstance(tokens, dict) or not isinstance(tokens.get("access_token"), str):
        raise RuntimeError(text("setup_error"))
    headers["Authorization"] = "Bearer " + tokens["access_token"]
    for path, payload in [
        ("/api/setup/instance", {"instance_name": name, "instance_description": description}),
        ("/api/setup/complete", {"instance_private": private, "p2p_enabled": peer,
                                 "open_registrations": registrations, "max_upload_size_mb": limit}),
    ]:
        code, _, _ = request(path, payload)
        if code not in (200, 201, 204):
            if status()["setup_complete"]:
                return {"setupComplete": True, "alreadyComplete": True}
            raise RuntimeError(text("setup_error"))
    if not status()["setup_complete"]:
        raise RuntimeError(text("setup_error"))
    return {"setupComplete": True, "alreadyComplete": False}
