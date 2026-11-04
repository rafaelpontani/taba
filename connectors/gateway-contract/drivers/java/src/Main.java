import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.net.URI;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.MessageDigest;
import java.util.*;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * The Java driver of the gateway contract tests (connectors/gateway-contract/README.md): reads the scenario files and
 * runs each line through the AWS SDK for Java (v2 or v1, see Ops), the way an application would. Prints one RESULT line
 * per scenario line.
 */
public class Main {
    static final Map<String, String> ARGS = new HashMap<>();
    static final List<String> SCENARIOS = new ArrayList<>();
    static final Map<String, String[]> CREDS = new HashMap<>();

    /** A result: the fields the expectations read, and the handles of the messages received (for the batch deletes). */
    static class Result {
        final Map<String, String> m = new HashMap<>();
        final List<String> handles = new ArrayList<>();

        Result with(String... kv) {
            for (int i = 0; i + 1 < kv.length; i += 2) m.put(kv[i], kv[i + 1]);
            return this;
        }
    }

    /** One message received, SDK-independent. */
    static class Msg {
        String id, body, handle, md5;
        Map<String, String> attributes = new HashMap<>();
        /** name -> {type, value} (a binary value is "b64:...") */
        Map<String, String[]> msgAttributes = new LinkedHashMap<>();
    }

    static String arg(String k, String d) {
        return ARGS.getOrDefault(k, d);
    }

    static boolean reachable(String url) {
        URI u = URI.create(url);
        try (Socket s = new Socket()) {
            s.connect(new InetSocketAddress(u.getHost(), u.getPort() < 0 ? 80 : u.getPort()), 3000);
            return true;
        } catch (IOException e) {
            return false;
        }
    }

    // ---- the scenario language ----------------------------------------------------------------------------------
    static final Pattern VARIABLE = Pattern.compile("\\{p\\}|\\{lang\\}|\\{prev\\}|\\$([A-Za-z0-9_]+)\\.([A-Za-z0-9_]+)");
    static final Pattern EXPECT = Pattern.compile("^([A-Za-z0-9_.]+)(>=|<=|!=|=|~)(.*)$", Pattern.DOTALL);

    static String unquote(String s) {
        try {
            return URLDecoder.decode(s.replace("+", "%2B"), "UTF-8");
        } catch (Exception e) {
            return s;
        }
    }

    static String expand(String value, Map<String, Result> vars) {
        StringBuilder out = new StringBuilder();
        int last = 0;
        Matcher m = VARIABLE.matcher(value);
        while (m.find()) {
            out.append(unquote(value.substring(last, m.start())));
            switch (m.group()) {
                case "{p}": out.append(arg("prefix", "")); break;
                case "{lang}": out.append(arg("lang", "")); break;
                case "{prev}": out.append(arg("prev", "")); break;
                default:
                    Result r = vars.get(m.group(1));
                    if (r != null) out.append(r.m.getOrDefault(m.group(2), ""));
            }
            last = m.end();
        }
        return out.append(unquote(value.substring(last))).toString();
    }

    static String[] pair(String s, char c) {
        int k = s.indexOf(c);
        return k < 0 ? new String[] {s, ""} : new String[] {s.substring(0, k), s.substring(k + 1)};
    }

    static String check(Result r, List<String[]> expects) {
        for (String[] e : expects) {
            String key = e[0], op = e[1], want = e[2];
            String got = r.m.getOrDefault(key, "");
            boolean ok;
            switch (op) {
                case "=": ok = want.equals("*") ? !got.isEmpty() : got.equals(want); break;
                case "!=": ok = !got.equals(want); break;
                case "~": ok = got.contains(want); break;
                case ">=": ok = num(got) >= num(want); break;
                default: ok = num(got) <= num(want);
            }
            if (!ok) return key + op + want + " but was '" + (got.length() > 200 ? got.substring(0, 200) : got) + "'";
        }
        return null;
    }

    static double num(String s) {
        try {
            return Double.parseDouble(s);
        } catch (NumberFormatException e) {
            return Double.NaN;
        }
    }

    // ---- helpers shared by the SDK flavours ------------------------------------------------------------------------
    static String md5(String s) {
        try {
            byte[] d = MessageDigest.getInstance("MD5").digest(s.getBytes(StandardCharsets.UTF_8));
            StringBuilder sb = new StringBuilder();
            for (byte b : d) sb.append(String.format("%02x", b));
            return sb.toString();
        } catch (Exception e) {
            throw new IllegalStateException(e);
        }
    }

    static Map<String, String> pairs(String t) {
        Map<String, String> d = new LinkedHashMap<>();
        if (t == null || t.isEmpty()) return d;
        for (String item : t.split(",")) {
            String[] kv = pair(item, ':');
            d.put(kv[0], kv[1]);
        }
        return d;
    }

    /** "name:Type:value" items. */
    static List<String[]> attrItems(String t) {
        List<String[]> out = new ArrayList<>();
        if (t == null || t.isEmpty()) return out;
        for (String item : t.split(",")) {
            int a = item.indexOf(':'), b = item.indexOf(':', a + 1);
            out.add(new String[] {item.substring(0, a), item.substring(a + 1, b), item.substring(b + 1)});
        }
        return out;
    }

    static byte[] binary(String v) {
        return Base64.getDecoder().decode(v.substring(4));
    }

    static String lastSegment(String url) {
        String u = url.replaceAll("/+$", "");
        return u.substring(u.lastIndexOf('/') + 1);
    }

    static String redrive(Map<String, String> p) {
        return "{\"deadLetterTargetArn\":\"" + p.get("redriveArn") + "\",\"maxReceiveCount\":" + p.get("redriveMax") + "}";
    }

    static String policy(Map<String, String> p) {
        return "{\"Version\":\"2012-10-17\",\"Statement\":[{\"Effect\":\"Allow\",\"Principal\":{\"Service\":\"sns.amazonaws.com\"},"
            + "\"Action\":\"sqs:SendMessage\",\"Resource\":\"" + p.get("queueArn") + "\",\"Condition\":{\"ArnEquals\":{\"aws:SourceArn\":\""
            + p.get("topic") + "\"}}}]}";
    }

    static Result received(List<Msg> msgs, long elapsed) {
        Result r = new Result();
        List<String> bodies = new ArrayList<>();
        for (Msg m : msgs) {
            bodies.add(m.body);
            r.handles.add(m.handle);
        }
        r.with("count", String.valueOf(msgs.size()), "elapsedMs", String.valueOf(elapsed), "bodies", String.join(",", bodies));
        Collections.sort(bodies);
        r.m.put("sorted", String.join(",", bodies));
        if (!msgs.isEmpty()) {
            Msg m = msgs.get(0);
            r.with("body", m.body, "id", m.id, "handle", m.handle, "md5", m.md5, "md5Ok", String.valueOf(md5(m.body).equals(m.md5)),
                "size", String.valueOf(m.body.codePointCount(0, m.body.length())));
            for (Map.Entry<String, String> e : m.attributes.entrySet()) r.m.put("attr_" + e.getKey(), e.getValue());
            msgAttrs(m, r.m);
            r.m.putAll(Json.topLevelStrings(m.body));
        }
        return r;
    }

    static void msgAttrs(Msg m, Map<String, String> into) {
        for (Map.Entry<String, String[]> e : m.msgAttributes.entrySet()) {
            into.put("msgattr_" + e.getKey(), e.getValue()[1]);
            into.put("msgattrtype_" + e.getKey(), e.getValue()[0]);
        }
    }

    // ---- the run -------------------------------------------------------------------------------------------------------
    public static void main(String[] argv) throws Exception {
        for (int i = 0; i + 1 < argv.length; i += 2) {
            String key = argv[i].substring(2), value = argv[i + 1];
            if (key.equals("scenario")) SCENARIOS.add(value);
            else if (key.equals("cred")) {
                String[] nv = pair(value, '=');
                CREDS.put(nv[0], pair(nv[1], ':'));
            } else ARGS.put(key, value);
        }
        Ops ops = (Ops) Class.forName(arg("flavor", "v2").equals("v1") ? "OpsV1" : "OpsV2").getDeclaredConstructor().newInstance();
        boolean unreachable = !(reachable(arg("sqs-endpoint", "")) && reachable(arg("sns-endpoint", "")));
        int pass = 0, fail = 0, connect = 0;
        Map<String, Result> vars = new HashMap<>();
        for (String scenario : SCENARIOS) {
            Path path = Paths.get(scenario);
            List<String> lines = Files.readAllLines(path, StandardCharsets.UTF_8);
            for (int n = 0; n < lines.size(); n++) {
                String text = lines.get(n).trim();
                if (text.isEmpty() || text.startsWith("#")) continue;
                String status = "PASS", detail = "", opName = text.split(" ")[0];
                try {
                    int at = text.indexOf(" => ");
                    String head = at < 0 ? text : text.substring(0, at), tail = at < 0 ? "" : text.substring(at + 4);
                    String[] parts = head.trim().split("\\s+");
                    opName = parts[0];
                    Map<String, String> p = new HashMap<>();
                    for (int k = 1; k < parts.length; k++) {
                        String[] kv = pair(parts[k], '=');
                        p.put(kv[0], expand(kv[1], vars));
                    }
                    List<String[]> expects = new ArrayList<>();
                    for (String token : tail.trim().split("\\s+")) {
                        if (token.isEmpty()) continue;
                        Matcher m = EXPECT.matcher(token);
                        if (!m.matches()) throw new IllegalArgumentException("bad expectation " + token);
                        expects.add(new String[] {m.group(1), m.group(2), expand(m.group(3), vars)});
                    }
                    String expected = null;
                    for (String[] e : expects) if (e[0].equals("error")) expected = e[2];
                    try {
                        Result r = ops.run(opName, p, vars);
                        if (expected != null) {
                            status = "FAIL";
                            detail = "expected error " + expected + " but the call worked";
                        } else {
                            String problem = check(r, expects);
                            if (problem != null) {
                                status = "FAIL";
                                detail = problem;
                            }
                        }
                        if (p.containsKey("as")) vars.put(p.get("as"), r);
                    } catch (Throwable t) {
                        String code = ops.errorCode(t);
                        if (code == null) throw t;
                        code = code.substring(code.lastIndexOf('#') + 1);
                        if (expected == null || !Arrays.asList(expected.split("\\|")).contains(code)) {
                            status = "FAIL";
                            String msg = String.valueOf(t.getMessage());
                            detail = "error " + code + ": " + (msg.length() > 160 ? msg.substring(0, 160) : msg);
                        }
                    }
                } catch (Throwable t) {
                    if (unreachable) {
                        status = "CONNECT";
                        detail = t.getClass().getSimpleName();
                    } else {
                        status = "FAIL";
                        String msg = String.valueOf(t.getMessage());
                        detail = t.getClass().getSimpleName() + ": " + (msg.length() > 200 ? msg.substring(0, 200) : msg);
                    }
                }
                switch (status) {
                    case "PASS": pass++; break;
                    case "FAIL": fail++; break;
                    default: connect++;
                }
                System.out.println("RESULT " + status + " " + path.getFileName() + ":" + (n + 1) + " " + opName + (detail.isEmpty() ? "" : " :: " + detail));
            }
        }
        System.out.println("SUMMARY lang=" + arg("lang", "java") + " variant=" + arg("variant", "") + " sdk=" + ops.sdkName()
            + " pass=" + pass + " fail=" + fail + " connect=" + connect);
        System.exit(0);
    }
}
