"""Read historical deployment-agent diagnostics in the English installer."""

import re
from string import Formatter

if __package__:
    from .diagnostic_strings import LEGACY_FRENCH
else:
    from diagnostic_strings import LEGACY_FRENCH


def _pattern(template):
    parts = []
    for literal, field, _, _ in Formatter().parse(template):
        parts.append(re.escape(literal))
        if field:
            value = r"\d+" if field in ("i", "total", "port") else r".+?"
            parts.append(f"(?P<{field}>{value})")
    return re.compile("".join(parts))


_DYNAMIC = [(_pattern(source), translation) for source, translation in LEGACY_FRENCH.items() if "{" in source]


def english(message):
    """Preserve paths and raw external details when reading previous agents' messages."""
    if not isinstance(message, str):
        return message
    if message in LEGACY_FRENCH:
        return LEGACY_FRENCH[message]
    for pattern, translation in _DYNAMIC:
        match = pattern.fullmatch(message)
        if match:
            return translation.format(**match.groupdict())
    return message


def localize(code, message):
    """Compatibility with callers from editions that accepted a language argument."""
    return english(message)
