using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace Taba
{
    /// <summary>Malformed JSON in the server's response.</summary>
    internal sealed class JsonException : Exception
    {
        public JsonException(int position, string message)
            : base("invalid JSON at position " + position.ToString(CultureInfo.InvariantCulture) + ": " + message)
        {
            Position = position;
        }

        public int Position { get; }
    }

    /// <summary>
    /// Minimal JSON reader (doc 19, section 18.5). Walks the text without building a tree: it can skip
    /// any value, hands out an object's fields as name → text-span pairs and decodes
    /// a JSON string. A message's <c>body</c> is any JSON, so the connector returns the
    /// text span without interpreting it.
    /// </summary>
    internal static class Json
    {
        /// <summary>Maximum nesting accepted; above it the document is refused instead of overflowing the stack.</summary>
        public const int MaxDepth = 512;

        private static JsonException Fail(int position, string message)
        {
            return new JsonException(position, message);
        }

        private static int Ws(string s, int i)
        {
            while (i < s.Length && (s[i] == ' ' || s[i] == '\t' || s[i] == '\n' || s[i] == '\r'))
            {
                i++;
            }
            return i;
        }

        /// <summary>Checks that the whole of <paramref name="s"/> is a single valid JSON value.</summary>
        public static void Validate(string s)
        {
            Finish(s, Skip(s, 0, 0));
        }

        /// <summary>Skips the value that starts at <paramref name="start"/> and returns the index right after it.</summary>
        public static int SkipValue(string s, int start)
        {
            return Skip(s, start, 0);
        }

        private static int Skip(string s, int i, int depth)
        {
            if (depth > MaxDepth)
            {
                throw Fail(i, "nesting too deep");
            }
            i = Ws(s, i);
            if (i >= s.Length)
            {
                throw Fail(i, "unexpected end");
            }
            switch (s[i])
            {
                case '{':
                    i = Ws(s, i + 1);
                    if (i < s.Length && s[i] == '}')
                    {
                        return i + 1;
                    }
                    while (true)
                    {
                        i = Ws(s, SkipString(s, i));
                        if (i >= s.Length || s[i] != ':')
                        {
                            throw Fail(i, "esperava ':'");
                        }
                        i = Ws(s, Skip(s, i + 1, depth + 1));
                        if (i < s.Length && s[i] == ',')
                        {
                            i = Ws(s, i + 1);
                        }
                        else if (i < s.Length && s[i] == '}')
                        {
                            return i + 1;
                        }
                        else
                        {
                            throw Fail(i, "expected ',' or '}'");
                        }
                    }
                case '[':
                    i = Ws(s, i + 1);
                    if (i < s.Length && s[i] == ']')
                    {
                        return i + 1;
                    }
                    while (true)
                    {
                        i = Ws(s, Skip(s, i, depth + 1));
                        if (i < s.Length && s[i] == ',')
                        {
                            i++;
                        }
                        else if (i < s.Length && s[i] == ']')
                        {
                            return i + 1;
                        }
                        else
                        {
                            throw Fail(i, "expected ',' or ']'");
                        }
                    }
                case '"':
                    return SkipString(s, i);
                case 't':
                    return Literal(s, i, "true");
                case 'f':
                    return Literal(s, i, "false");
                case 'n':
                    return Literal(s, i, "null");
                default:
                    if (s[i] == '-' || (s[i] >= '0' && s[i] <= '9'))
                    {
                        return SkipNumber(s, i);
                    }
                    throw Fail(i, "unexpected value");
            }
        }

        private static int Literal(string s, int i, string word)
        {
            if (string.CompareOrdinal(s, i, word, 0, word.Length) == 0)
            {
                return i + word.Length;
            }
            throw Fail(i, "invalid literal");
        }

        private static bool IsHex(char c)
        {
            return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f') || (c >= 'A' && c <= 'F');
        }

        private static int SkipString(string s, int i)
        {
            if (i >= s.Length || s[i] != '"')
            {
                throw Fail(i, "expected a string");
            }
            i++;
            while (true)
            {
                if (i >= s.Length)
                {
                    throw Fail(i, "unterminated string");
                }
                char c = s[i];
                if (c == '"')
                {
                    return i + 1;
                }
                if (c == '\\')
                {
                    char e = i + 1 < s.Length ? s[i + 1] : '\0';
                    if (e == '"' || e == '\\' || e == '/' || e == 'b' || e == 'f' || e == 'n' || e == 'r' || e == 't')
                    {
                        i += 2;
                    }
                    else if (e == 'u')
                    {
                        if (i + 6 <= s.Length && IsHex(s[i + 2]) && IsHex(s[i + 3]) && IsHex(s[i + 4]) && IsHex(s[i + 5]))
                        {
                            i += 6;
                        }
                        else
                        {
                            throw Fail(i, "invalid \\u escape");
                        }
                    }
                    else
                    {
                        throw Fail(i, "invalid escape");
                    }
                }
                else if (c < 0x20)
                {
                    throw Fail(i, "control character in string");
                }
                else
                {
                    i++;
                }
            }
        }

        private static int Digits(string s, int i)
        {
            while (i < s.Length && s[i] >= '0' && s[i] <= '9')
            {
                i++;
            }
            return i;
        }

        private static int SkipNumber(string s, int i)
        {
            if (s[i] == '-')
            {
                i++;
            }
            if (i < s.Length && s[i] == '0')
            {
                i++;
            }
            else if (i < s.Length && s[i] >= '1' && s[i] <= '9')
            {
                i = Digits(s, i);
            }
            else
            {
                throw Fail(i, "invalid number");
            }
            if (i < s.Length && s[i] == '.')
            {
                int next = Digits(s, i + 1);
                if (next == i + 1)
                {
                    throw Fail(i, "invalid number");
                }
                i = next;
            }
            if (i < s.Length && (s[i] == 'e' || s[i] == 'E'))
            {
                i++;
                if (i < s.Length && (s[i] == '+' || s[i] == '-'))
                {
                    i++;
                }
                int next = Digits(s, i);
                if (next == i)
                {
                    throw Fail(i, "invalid number");
                }
                i = next;
            }
            return i;
        }

        private static void Finish(string s, int i)
        {
            if (Ws(s, i) != s.Length)
            {
                throw Fail(Ws(s, i), "text after the document");
            }
        }

        /// <summary>The fields of an object, with the name still quoted and the value as a text span.</summary>
        private static List<KeyValuePair<string, string>> RawFields(string s)
        {
            var fields = new List<KeyValuePair<string, string>>();
            int i = Ws(s, 0);
            if (i >= s.Length || s[i] != '{')
            {
                throw Fail(i, "expected an object");
            }
            i = Ws(s, i + 1);
            if (i < s.Length && s[i] == '}')
            {
                Finish(s, i + 1);
                return fields;
            }
            while (true)
            {
                int keyEnd = SkipString(s, i);
                string key = s.Substring(i, keyEnd - i);
                i = Ws(s, keyEnd);
                if (i >= s.Length || s[i] != ':')
                {
                    throw Fail(i, "esperava ':'");
                }
                int valueStart = Ws(s, i + 1);
                int valueEnd = Skip(s, valueStart, 1);
                fields.Add(new KeyValuePair<string, string>(key, s.Substring(valueStart, valueEnd - valueStart)));
                i = Ws(s, valueEnd);
                if (i < s.Length && s[i] == ',')
                {
                    i = Ws(s, i + 1);
                }
                else if (i < s.Length && s[i] == '}')
                {
                    Finish(s, i + 1);
                    return fields;
                }
                else
                {
                    throw Fail(i, "expected ',' or '}'");
                }
            }
        }

        /// <summary>The fields of an object: decoded name and the text span of each value.</summary>
        public static List<KeyValuePair<string, string>> ObjectFields(string s)
        {
            var fields = RawFields(s);
            for (int n = 0; n < fields.Count; n++)
            {
                fields[n] = new KeyValuePair<string, string>(DecodeString(fields[n].Key), fields[n].Value);
            }
            return fields;
        }

        /// <summary>The items of a list, each one as a text span.</summary>
        public static List<string> ArrayItems(string s)
        {
            var items = new List<string>();
            int i = Ws(s, 0);
            if (i >= s.Length || s[i] != '[')
            {
                throw Fail(i, "expected a list");
            }
            i = Ws(s, i + 1);
            if (i < s.Length && s[i] == ']')
            {
                Finish(s, i + 1);
                return items;
            }
            while (true)
            {
                int end = Skip(s, i, 1);
                items.Add(s.Substring(i, end - i).Trim());
                i = Ws(s, end);
                if (i < s.Length && s[i] == ',')
                {
                    i = Ws(s, i + 1);
                }
                else if (i < s.Length && s[i] == ']')
                {
                    Finish(s, i + 1);
                    return items;
                }
                else
                {
                    throw Fail(i, "expected ',' or ']'");
                }
            }
        }

        private static int Hex4(string s, int at)
        {
            if (at + 4 > s.Length)
            {
                throw Fail(at, "invalid \\u escape");
            }
            int value = 0;
            for (int n = 0; n < 4; n++)
            {
                char c = s[at + n];
                if (!IsHex(c))
                {
                    throw Fail(at, "invalid \\u escape");
                }
                value = (value * 16) + Convert.ToInt32(c.ToString(), 16);
            }
            return value;
        }

        /// <summary>Decodes a JSON string (<paramref name="raw"/> includes the quotes): escapes, <c>\uXXXX</c> and surrogate pairs.</summary>
        public static string DecodeString(string raw)
        {
            raw = raw.Trim();
            if (raw.Length < 2 || raw[0] != '"' || raw[raw.Length - 1] != '"')
            {
                throw Fail(0, "expected a string");
            }
            var sb = new StringBuilder(raw.Length);
            int end = raw.Length - 1;
            int i = 1;
            while (i < end)
            {
                char c = raw[i];
                if (c != '\\')
                {
                    sb.Append(c);
                    i++;
                    continue;
                }
                if (i + 1 >= end)
                {
                    throw Fail(i, "escape incompleto");
                }
                char e = raw[i + 1];
                i += 2;
                switch (e)
                {
                    case '"': sb.Append('"'); break;
                    case '\\': sb.Append('\\'); break;
                    case '/': sb.Append('/'); break;
                    case 'b': sb.Append('\b'); break;
                    case 'f': sb.Append('\f'); break;
                    case 'n': sb.Append('\n'); break;
                    case 'r': sb.Append('\r'); break;
                    case 't': sb.Append('\t'); break;
                    case 'u':
                        int first = Hex4(raw, i);
                        i += 4;
                        if (first >= 0xD800 && first < 0xDC00)
                        {
                            // Surrogate pair: the second escape must follow right away.
                            bool paired = false;
                            if (i + 6 <= end && raw[i] == '\\' && raw[i + 1] == 'u')
                            {
                                int second = Hex4(raw, i + 2);
                                if (second >= 0xDC00 && second < 0xE000)
                                {
                                    sb.Append((char)first).Append((char)second);
                                    i += 6;
                                    paired = true;
                                }
                            }
                            if (!paired)
                            {
                                sb.Append('�');
                            }
                        }
                        else if (first >= 0xDC00 && first < 0xE000)
                        {
                            sb.Append('�');
                        }
                        else
                        {
                            sb.Append((char)first);
                        }
                        break;
                    default:
                        throw Fail(i - 2, "invalid escape");
                }
            }
            return sb.ToString();
        }

        /// <summary>Writes <paramref name="s"/> as a JSON string, in quotes.</summary>
        public static string Quote(string s)
        {
            var sb = new StringBuilder(s.Length + 2);
            sb.Append('"');
            foreach (char c in s)
            {
                switch (c)
                {
                    case '"': sb.Append("\\\""); break;
                    case '\\': sb.Append("\\\\"); break;
                    case '\n': sb.Append("\\n"); break;
                    case '\r': sb.Append("\\r"); break;
                    case '\t': sb.Append("\\t"); break;
                    default:
                        if (c < 0x20)
                        {
                            sb.Append("\\u").Append(((int)c).ToString("x4", CultureInfo.InvariantCulture));
                        }
                        else
                        {
                            sb.Append(c);
                        }
                        break;
                }
            }
            return sb.Append('"').ToString();
        }
    }

    /// <summary>The fields of a JSON object, searchable by name. Unknown fields are simply ignored.</summary>
    internal sealed class JsonFields
    {
        private readonly List<KeyValuePair<string, string>> _fields;

        private JsonFields(List<KeyValuePair<string, string>> fields)
        {
            _fields = fields;
        }

        public static JsonFields Parse(string s)
        {
            return new JsonFields(Json.ObjectFields(s));
        }

        /// <summary>The field's text span, or <c>null</c> if it does not exist.</summary>
        public string Raw(string name)
        {
            foreach (var field in _fields)
            {
                if (field.Key == name)
                {
                    return field.Value;
                }
            }
            return null;
        }

        /// <summary>The field as text, or <c>null</c> if it does not exist or is not a string.</summary>
        public string String(string name)
        {
            string raw = Raw(name);
            return raw != null && raw.StartsWith("\"", StringComparison.Ordinal) ? Json.DecodeString(raw) : null;
        }

        /// <summary>The field as an unsigned integer, or <c>null</c>.</summary>
        public long? Integer(string name)
        {
            long value;
            string raw = Raw(name);
            return raw != null && long.TryParse(raw, NumberStyles.None, CultureInfo.InvariantCulture, out value)
                ? (long?)value
                : null;
        }

        /// <summary>The field as a boolean, or <c>null</c>.</summary>
        public bool? Boolean(string name)
        {
            string raw = Raw(name);
            return raw == "true" ? (bool?)true : raw == "false" ? (bool?)false : null;
        }
    }
}
