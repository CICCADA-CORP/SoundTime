"""Adapt the NAS Compose model for Container Station's stand-alone YAML editor."""

import copy
import json
import posixpath
import re


# Only the interpolations emitted by the project exporter are accepted here.
VARIABLE = re.compile(r"\$\$|\$\{([A-Z][A-Z0-9_]*)(:\?|:-([^{}]*))?\}")


def _expand(value, variables):
    def replace(match):
        if match[0] == "$$":
            return "$"
        name = match[1]
        if variables.get(name):
            return variables[name]
        if match[2] and match[2].startswith(":-"):
            return match[3]
        raise ValueError(f"Missing QNAP project variable {name}.")

    return VARIABLE.sub(replace, value)


def _env_line(key, value):
    if any(ch in value for ch in ("\x00", "\r", "\n")):
        raise ValueError("A project variable contains a line break.")
    # Quoted Compose env-file values preserve spaces, #, quotes and backslashes.
    # $$ protects literal dollar signs from Compose variable interpolation.
    escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("$", "$$")
    return f'{key}="{escaped}"\n'


def variant(settings, model, env_values):
    """Return a private env file per credential-bearing service and one GUI YAML."""
    compose = copy.deepcopy(model)
    files = {}
    for service in ("postgres", "backend"):
        config = compose["services"][service]
        options = config.pop("environment")
        path = f"qnap/{service}.env"
        full_path = posixpath.join(settings["directory"], path).replace("$", "$$")
        config["env_file"] = [full_path]
        files[path] = "".join(_env_line(k, _expand(v, env_values))
                              for k, v in options.items()).encode()
    for port in compose["services"]["nginx"]["ports"]:
        port["host_ip"] = _expand(port["host_ip"], env_values)
    # JSON is accepted by the Compose YAML parser and preserves string types.
    files["qnap-compose.yaml"] = (json.dumps(compose, ensure_ascii=False, indent=2) + "\n").encode()
    return files
