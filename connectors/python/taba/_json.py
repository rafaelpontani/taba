"""
The minimal JSON reader (doc 21, "No dependencies"): walks the text once and delivers the fields of an object as
name -> text fragment and the items of an array as text fragments, without interpreting them. That keeps the
message `body` exactly as the server sent it (`json.loads` would turn `1.0` into a float and reformat the text).
"""

import json
import re
from typing import Dict, List, Optional

_WHITESPACE = " \t\r\n"


class JsonError(ValueError):
    pass


def _skip(s: str, i: int) -> int:
    n = len(s)
    while i < n and s[i] in _WHITESPACE:
        i += 1
    return i


def _scan_string(s: str, i: int) -> int:
    i += 1
    n = len(s)
    while i < n:
        c = s[i]
        if c == "\\":
            i += 1
        elif c == '"':
            return i + 1
        i += 1
    raise JsonError("unterminated string")


def _scan_value(s: str, i: int) -> int:
    """The index right after the value that starts at `i` (iterative: deep nesting does not use the stack)."""
    n = len(s)
    if i >= n:
        raise JsonError("value expected")
    first = s[i]
    if first == '"':
        return _scan_string(s, i)
    if first in "{[":
        depth = 0
        while i < n:
            c = s[i]
            if c == '"':
                i = _scan_string(s, i) - 1
            elif c in "{[":
                depth += 1
            elif c in "}]":
                depth -= 1
                if depth == 0:
                    return i + 1
            i += 1
        raise JsonError("unterminated object or array")
    start = i
    while i < n and s[i] not in ",]} \t\r\n":
        i += 1
    if i == start:
        raise JsonError("value expected")
    return i


def decode_string(fragment: str) -> str:
    """Decodes a string fragment (with the quotes) into text."""
    if not fragment.startswith('"'):
        raise JsonError("string expected")
    try:
        return json.loads(fragment)
    except ValueError:
        raise JsonError("invalid string")


def array_items(fragment: str) -> List[str]:
    """The items of an array fragment, each as a text fragment."""
    s = fragment
    i = _skip(s, 0)
    if i >= len(s) or s[i] != "[":
        raise JsonError("array expected")
    items: List[str] = []
    i = _skip(s, i + 1)
    if i < len(s) and s[i] == "]":
        return items
    while True:
        i = _skip(s, i)
        end = _scan_value(s, i)
        items.append(s[i:end])
        i = _skip(s, end)
        if i < len(s) and s[i] == ",":
            i += 1
        elif i < len(s) and s[i] == "]":
            return items
        else:
            raise JsonError('"," or "]" expected')


class JsonFields:
    """The fields of one JSON object."""

    def __init__(self, fields: Dict[str, str]) -> None:
        self._fields = fields

    @classmethod
    def parse(cls, text: str) -> "JsonFields":
        s = text
        n = len(s)
        fields: Dict[str, str] = {}
        i = _skip(s, 0)
        if i >= n or s[i] != "{":
            raise JsonError("object expected")
        i = _skip(s, i + 1)
        if i >= n:
            raise JsonError("unterminated object")
        if s[i] != "}":
            while True:
                i = _skip(s, i)
                if i >= n or s[i] != '"':
                    raise JsonError("field name expected")
                key_end = _scan_string(s, i)
                key = decode_string(s[i:key_end])
                i = _skip(s, key_end)
                if i >= n or s[i] != ":":
                    raise JsonError('":" expected')
                i = _skip(s, i + 1)
                end = _scan_value(s, i)
                fields[key] = s[i:end]
                i = _skip(s, end)
                if i < n and s[i] == ",":
                    i += 1
                elif i < n and s[i] == "}":
                    break
                else:
                    raise JsonError('"," or "}" expected')
        if _skip(s, i + 1) != n:
            raise JsonError("unexpected text after the object")
        return cls(fields)

    def raw(self, name: str) -> Optional[str]:
        """The field's text fragment, or `None` when absent or `null`."""
        value = self._fields.get(name)
        return None if value is None or value == "null" else value

    def string(self, name: str) -> Optional[str]:
        value = self.raw(name)
        return None if value is None else decode_string(value)

    def boolean(self, name: str) -> Optional[bool]:
        value = self.raw(name)
        if value is None:
            return None
        if value in ("true", "false"):
            return value == "true"
        raise JsonError('field "%s" is not a boolean' % name)

    def integer(self, name: str) -> Optional[int]:
        value = self.raw(name)
        if value is None:
            return None
        if not re.fullmatch(r"-?[0-9]+", value):
            raise JsonError('field "%s" is not an integer' % name)
        return int(value)
