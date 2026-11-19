"""Locale selection shared by every terminal entry point."""
import locale
import os
import sys

LANGUAGES = {"en": "English", "fr": "Français", "es": "Español", "ru": "Русский", "zh": "中文"}
_ORDER = tuple(LANGUAGES)


def _code(value):
    return (value or "").strip().lower().split(".")[0].split("@")[0].replace("_", "-").split("-")[0]


def language(explicit=None):
    """Prefer the selected language, then terminal/OS preferences, then English."""
    if explicit:
        return _code(explicit) if _code(explicit) in LANGUAGES else "en"
    candidates = [os.environ.get("SOUNDTIME_LANG"), os.environ.get("LC_ALL"),
                  os.environ.get("LC_MESSAGES"), *os.environ.get("LANGUAGE", "").split(":"),
                  os.environ.get("LANG")]
    if sys.platform == "win32":
        import ctypes
        candidates.append(locale.windows_locale.get(ctypes.windll.kernel32.GetUserDefaultUILanguage()))
    try:
        candidates.append(locale.getlocale()[0])
    except (ValueError, TypeError):
        pass
    return next((code for code in map(_code, candidates) if code in LANGUAGES), "en")


def text(code, rows, key, **values):
    return rows[key][_ORDER.index(language(code))].format(**values)
