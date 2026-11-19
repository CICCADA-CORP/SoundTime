"""Localize engine and older SSH-agent messages without rewriting external details."""

import re
from string import Formatter

if __package__:
    from . import i18n
    from .diagnostic_strings import MESSAGES
else:
    import i18n
    from diagnostic_strings import MESSAGES


# French is the wire format used by earlier versions of the deployment agent.
_ROWS = {row[1]: row for row in MESSAGES}


def _pattern(template):
    parts = []
    for literal, field, _, _ in Formatter().parse(template):
        parts.append(re.escape(literal))
        if field:
            value = r"\d+" if field in ("i", "total", "port") else r".+?"
            parts.append(f"(?P<{field}>{value})")
    return re.compile("".join(parts))


_DYNAMIC = [(template, _pattern(template)) for template in _ROWS if "{" in template]


def localize(code, message):
    """Translate known diagnostics while preserving paths, hosts and service output."""
    if not isinstance(message, str):
        return message
    if message == "Les réglages sont incomplets. Rechargez la page de l’assistant.":
        message = "Les réglages sont incomplets. Relancez l’installateur et réessayez."
    if i18n.language(code) == "fr":
        return message
    if message in _ROWS and "{" not in message:
        return i18n.text(code, _ROWS, message)
    for template, pattern in _DYNAMIC:
        match = pattern.fullmatch(message)
        if match:
            return i18n.text(code, _ROWS, template, **match.groupdict())
    return message
