'use strict';

/**
 * The minimal JSON reader (doc 21, "No dependencies"): walks the text once, delivers the fields of an
 * object as name -> text fragment and the items of an array as text fragments, without interpreting them.
 * That keeps the message `body` exactly as the server sent it (`JSON.parse` would turn `1.0` into `1`
 * and lose integers above 2^53).
 */
class JsonError extends Error {}

function skipWhitespace(s, i) {
  while (i < s.length) {
    const c = s.charCodeAt(i);
    if (c !== 0x20 && c !== 0x09 && c !== 0x0a && c !== 0x0d) {
      break;
    }
    i++;
  }
  return i;
}

function scanString(s, i) {
  for (i++; i < s.length; i++) {
    const c = s[i];
    if (c === '\\') {
      i++;
    } else if (c === '"') {
      return i + 1;
    }
  }
  throw new JsonError('unterminated string');
}

/** The index right after the value that starts at `i` (iterative: deep nesting does not use the stack). */
function scanValue(s, i) {
  if (i >= s.length) {
    throw new JsonError('value expected');
  }
  const first = s[i];
  if (first === '"') {
    return scanString(s, i);
  }
  if (first === '{' || first === '[') {
    let depth = 0;
    for (; i < s.length; i++) {
      const c = s[i];
      if (c === '"') {
        i = scanString(s, i) - 1;
      } else if (c === '{' || c === '[') {
        depth++;
      } else if (c === '}' || c === ']') {
        depth--;
        if (depth === 0) {
          return i + 1;
        }
      }
    }
    throw new JsonError('unterminated object or array');
  }
  const start = i;
  while (i < s.length && ',]} \t\r\n'.indexOf(s[i]) < 0) {
    i++;
  }
  if (i === start) {
    throw new JsonError('value expected');
  }
  return i;
}

/** Decodes a string fragment (with the quotes) into text. */
function decodeString(fragment) {
  if (fragment[0] !== '"') {
    throw new JsonError('string expected');
  }
  try {
    return JSON.parse(fragment);
  } catch (e) {
    throw new JsonError('invalid string');
  }
}

/** The items of an array fragment, each as a text fragment. */
function arrayItems(fragment) {
  const s = fragment;
  let i = skipWhitespace(s, 0);
  if (s[i] !== '[') {
    throw new JsonError('array expected');
  }
  const items = [];
  i = skipWhitespace(s, i + 1);
  if (s[i] === ']') {
    return items;
  }
  for (;;) {
    i = skipWhitespace(s, i);
    const end = scanValue(s, i);
    items.push(s.slice(i, end));
    i = skipWhitespace(s, end);
    if (s[i] === ',') {
      i++;
    } else if (s[i] === ']') {
      return items;
    } else {
      throw new JsonError('"," or "]" expected');
    }
  }
}

/** The fields of one JSON object. */
class JsonFields {
  constructor(map) {
    this._map = map;
  }

  static parse(text) {
    const s = String(text);
    const map = new Map();
    let i = skipWhitespace(s, 0);
    if (s[i] !== '{') {
      throw new JsonError('object expected');
    }
    i = skipWhitespace(s, i + 1);
    if (s[i] !== '}') {
      for (;;) {
        i = skipWhitespace(s, i);
        if (s[i] !== '"') {
          throw new JsonError('field name expected');
        }
        const keyEnd = scanString(s, i);
        const key = decodeString(s.slice(i, keyEnd));
        i = skipWhitespace(s, keyEnd);
        if (s[i] !== ':') {
          throw new JsonError('":" expected');
        }
        i = skipWhitespace(s, i + 1);
        const end = scanValue(s, i);
        map.set(key, s.slice(i, end));
        i = skipWhitespace(s, end);
        if (s[i] === ',') {
          i++;
        } else if (s[i] === '}') {
          break;
        } else {
          throw new JsonError('"," or "}" expected');
        }
      }
    }
    if (skipWhitespace(s, i + 1) !== s.length) {
      throw new JsonError('unexpected text after the object');
    }
    return new JsonFields(map);
  }

  /** The field's text fragment, or `null` when absent or `null`. */
  raw(name) {
    const value = this._map.get(name);
    return value === undefined || value === 'null' ? null : value;
  }

  string(name) {
    const value = this.raw(name);
    return value === null ? null : decodeString(value);
  }

  boolean(name) {
    const value = this.raw(name);
    if (value === null) {
      return null;
    }
    if (value === 'true' || value === 'false') {
      return value === 'true';
    }
    throw new JsonError(`field "${name}" is not a boolean`);
  }

  integer(name) {
    const value = this.raw(name);
    if (value === null) {
      return null;
    }
    if (!/^-?\d+$/.test(value)) {
      throw new JsonError(`field "${name}" is not an integer`);
    }
    return Number(value);
  }
}

module.exports = { JsonError, JsonFields, arrayItems, decodeString };
