package io.github.rafaelpontani.taba;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

/**
 * The minimal JSON reader (doc 21, "No dependencies"): walks the text once, delivers the fields of an object as
 * name to text fragment and the items of an array as text fragments, without interpreting them. The JDK has no
 * JSON library, and this keeps the message {@code body} exactly as the server sent it.
 */
final class Json {
    private Json() {
    }

    static final class JsonException extends RuntimeException {
        private static final long serialVersionUID = 1L;

        JsonException(String message) {
            super(message);
        }
    }

    private static int skip(String s, int i) {
        while (i < s.length()) {
            char c = s.charAt(i);
            if (c != ' ' && c != '\t' && c != '\n' && c != '\r') {
                break;
            }
            i++;
        }
        return i;
    }

    private static int scanString(String s, int i) {
        for (i++; i < s.length(); i++) {
            char c = s.charAt(i);
            if (c == '\\') {
                i++;
            } else if (c == '"') {
                return i + 1;
            }
        }
        throw new JsonException("unterminated string");
    }

    /** The index right after the value that starts at {@code i} (iterative: deep nesting does not use the stack). */
    private static int scanValue(String s, int i) {
        if (i >= s.length()) {
            throw new JsonException("value expected");
        }
        char first = s.charAt(i);
        if (first == '"') {
            return scanString(s, i);
        }
        if (first == '{' || first == '[') {
            int depth = 0;
            for (; i < s.length(); i++) {
                char c = s.charAt(i);
                if (c == '"') {
                    i = scanString(s, i) - 1;
                } else if (c == '{' || c == '[') {
                    depth++;
                } else if (c == '}' || c == ']') {
                    depth--;
                    if (depth == 0) {
                        return i + 1;
                    }
                }
            }
            throw new JsonException("unterminated object or array");
        }
        int start = i;
        while (i < s.length() && ",]} \t\r\n".indexOf(s.charAt(i)) < 0) {
            i++;
        }
        if (i == start) {
            throw new JsonException("value expected");
        }
        return i;
    }

    /** Decodes a string fragment (with the quotes) into text. */
    static String decodeString(String fragment) {
        if (fragment.isEmpty() || fragment.charAt(0) != '"' || fragment.charAt(fragment.length() - 1) != '"' || fragment.length() < 2) {
            throw new JsonException("string expected");
        }
        StringBuilder out = new StringBuilder(fragment.length());
        for (int i = 1; i < fragment.length() - 1; i++) {
            char c = fragment.charAt(i);
            if (c != '\\') {
                out.append(c);
                continue;
            }
            if (++i >= fragment.length() - 1) {
                throw new JsonException("invalid escape");
            }
            char e = fragment.charAt(i);
            switch (e) {
                case '"':
                case '\\':
                case '/':
                    out.append(e);
                    break;
                case 'b':
                    out.append('\b');
                    break;
                case 'f':
                    out.append('\f');
                    break;
                case 'n':
                    out.append('\n');
                    break;
                case 'r':
                    out.append('\r');
                    break;
                case 't':
                    out.append('\t');
                    break;
                case 'u':
                    if (i + 5 >= fragment.length()) {
                        throw new JsonException("invalid unicode escape");
                    }
                    try {
                        out.append((char) Integer.parseInt(fragment.substring(i + 1, i + 5), 16));
                    } catch (NumberFormatException ex) {
                        throw new JsonException("invalid unicode escape");
                    }
                    i += 4;
                    break;
                default:
                    throw new JsonException("invalid escape");
            }
        }
        return out.toString();
    }

    /** The JSON string literal for {@code text}, with the quotes. */
    static String quote(String text) {
        StringBuilder out = new StringBuilder(text.length() + 2).append('"');
        for (int i = 0; i < text.length(); i++) {
            char c = text.charAt(i);
            switch (c) {
                case '"':
                    out.append("\\\"");
                    break;
                case '\\':
                    out.append("\\\\");
                    break;
                case '\n':
                    out.append("\\n");
                    break;
                case '\r':
                    out.append("\\r");
                    break;
                case '\t':
                    out.append("\\t");
                    break;
                default:
                    if (c < 0x20) {
                        out.append(String.format("\\u%04x", (int) c));
                    } else {
                        out.append(c);
                    }
            }
        }
        return out.append('"').toString();
    }

    /** The items of an array fragment, each as a text fragment. */
    static List<String> arrayItems(String s) {
        int i = skip(s, 0);
        if (i >= s.length() || s.charAt(i) != '[') {
            throw new JsonException("array expected");
        }
        List<String> items = new ArrayList<>();
        i = skip(s, i + 1);
        if (i < s.length() && s.charAt(i) == ']') {
            return items;
        }
        while (true) {
            i = skip(s, i);
            int end = scanValue(s, i);
            items.add(s.substring(i, end));
            i = skip(s, end);
            if (i < s.length() && s.charAt(i) == ',') {
                i++;
            } else if (i < s.length() && s.charAt(i) == ']') {
                return items;
            } else {
                throw new JsonException("\",\" or \"]\" expected");
            }
        }
    }

    /** The fields of one JSON object. */
    static final class Fields {
        private final Map<String, String> map;

        private Fields(Map<String, String> map) {
            this.map = map;
        }

        static Fields parse(String s) {
            Map<String, String> map = new HashMap<>();
            int n = s.length();
            int i = skip(s, 0);
            if (i >= n || s.charAt(i) != '{') {
                throw new JsonException("object expected");
            }
            i = skip(s, i + 1);
            if (i >= n) {
                throw new JsonException("unterminated object");
            }
            if (s.charAt(i) != '}') {
                while (true) {
                    i = skip(s, i);
                    if (i >= n || s.charAt(i) != '"') {
                        throw new JsonException("field name expected");
                    }
                    int keyEnd = scanString(s, i);
                    String key = decodeString(s.substring(i, keyEnd));
                    i = skip(s, keyEnd);
                    if (i >= n || s.charAt(i) != ':') {
                        throw new JsonException("\":\" expected");
                    }
                    i = skip(s, i + 1);
                    int end = scanValue(s, i);
                    map.put(key, s.substring(i, end));
                    i = skip(s, end);
                    if (i < n && s.charAt(i) == ',') {
                        i++;
                    } else if (i < n && s.charAt(i) == '}') {
                        break;
                    } else {
                        throw new JsonException("\",\" or \"}\" expected");
                    }
                }
            }
            if (skip(s, i + 1) != n) {
                throw new JsonException("unexpected text after the object");
            }
            return new Fields(map);
        }

        /** The field's text fragment, or null when absent or {@code null}. */
        String raw(String name) {
            String value = map.get(name);
            return value == null || value.equals("null") ? null : value;
        }

        String string(String name) {
            String value = raw(name);
            return value == null ? null : decodeString(value);
        }

        Boolean bool(String name) {
            String value = raw(name);
            if (value == null) {
                return null;
            }
            if (value.equals("true") || value.equals("false")) {
                return Boolean.valueOf(value);
            }
            throw new JsonException("field \"" + name + "\" is not a boolean");
        }

        Long integer(String name) {
            String value = raw(name);
            if (value == null) {
                return null;
            }
            if (!value.matches("-?[0-9]+")) {
                throw new JsonException("field \"" + name + "\" is not an integer");
            }
            try {
                return Long.valueOf(value);
            } catch (NumberFormatException e) {
                throw new JsonException("field \"" + name + "\" is out of range");
            }
        }
    }
}
