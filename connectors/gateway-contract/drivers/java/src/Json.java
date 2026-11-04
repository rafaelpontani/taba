import java.util.HashMap;
import java.util.Map;

/** Just enough JSON to read the string fields of the top-level object of a body (the SNS envelope). */
final class Json {
    private final String s;
    private int i;

    private Json(String s) { this.s = s; }

    static Map<String, String> topLevelStrings(String body) {
        Map<String, String> out = new HashMap<>();
        try {
            Json j = new Json(body.trim());
            if (j.s.isEmpty() || j.s.charAt(0) != '{') return out;
            j.i = 1;
            j.ws();
            while (j.i < j.s.length() && j.s.charAt(j.i) != '}') {
                String key = j.string();
                j.ws();
                j.expect(':');
                j.ws();
                if (j.s.charAt(j.i) == '"') out.put("json_" + key, j.string());
                else j.skipValue();
                j.ws();
                if (j.s.charAt(j.i) == ',') {
                    j.i++;
                    j.ws();
                }
            }
        } catch (RuntimeException e) {
            // not JSON: no fields
        }
        return out;
    }

    private void ws() {
        while (i < s.length() && Character.isWhitespace(s.charAt(i))) i++;
    }

    private void expect(char c) {
        if (s.charAt(i++) != c) throw new IllegalStateException();
    }

    private String string() {
        expect('"');
        StringBuilder sb = new StringBuilder();
        while (s.charAt(i) != '"') {
            char c = s.charAt(i++);
            if (c != '\\') {
                sb.append(c);
                continue;
            }
            char e = s.charAt(i++);
            switch (e) {
                case 'n': sb.append('\n'); break;
                case 't': sb.append('\t'); break;
                case 'r': sb.append('\r'); break;
                case 'b': sb.append('\b'); break;
                case 'f': sb.append('\f'); break;
                case 'u': sb.append((char) Integer.parseInt(s.substring(i, i + 4), 16)); i += 4; break;
                default: sb.append(e);
            }
        }
        i++;
        return sb.toString();
    }

    private void skipValue() {
        int depth = 0;
        while (i < s.length()) {
            char c = s.charAt(i);
            if (c == '"') {
                string();
                continue;
            }
            if (c == '{' || c == '[') depth++;
            if (c == '}' || c == ']') {
                if (depth == 0) return;
                depth--;
            }
            if (c == ',' && depth == 0) return;
            i++;
        }
    }
}
