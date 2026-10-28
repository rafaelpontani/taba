import io.github.rafaelpontani.taba.ConsumeOptions;
import io.github.rafaelpontani.taba.Message;
import io.github.rafaelpontani.taba.PublishOptions;
import io.github.rafaelpontani.taba.PublishResponse;
import io.github.rafaelpontani.taba.Subscription;
import io.github.rafaelpontani.taba.TabaClient;
import io.github.rafaelpontani.taba.TabaConfig;
import io.github.rafaelpontani.taba.TabaException;
import io.github.rafaelpontani.taba.Topic;
import io.github.rafaelpontani.taba.TopicRequest;
import io.github.rafaelpontani.taba.TopicUpdate;
import java.io.ByteArrayOutputStream;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Paths;
import java.util.ArrayList;
import java.util.Base64;
import java.util.Collections;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.concurrent.CancellationException;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CountDownLatch;
import java.util.concurrent.TimeUnit;
import java.util.concurrent.atomic.AtomicBoolean;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
import java.util.stream.Collectors;

/**
 * The Java driver of the connector contract tests (connectors/contract/README.md): reads the scenario files and runs
 * each line against a real Taba through the connector, in the style of the language.
 *
 * <pre>
 * java -cp out ContractDriver --url http://host:8080 --token TOKEN --prefix ct-java-0101 --lang java \
 *     --scenario ../contract/scenarios/core.scn
 * </pre>
 */
public final class ContractDriver {
    private static final Map<String, String> ARGS = new HashMap<>();
    private static final List<String> SCENARIOS = new ArrayList<>();
    private static final Pattern VARIABLE = Pattern.compile("\\{p\\}|\\{lang\\}|\\$([A-Za-z0-9_]+)\\.([A-Za-z0-9_]+)");

    private interface Operation {
        Map<String, String> run(TabaClient client, Map<String, String> p) throws Exception;
    }

    private static final class Expect {
        final String key;
        final char op;
        final String value;

        Expect(String key, char op, String value) {
            this.key = key;
            this.op = op;
            this.value = value;
        }
    }

    // ---- the scenario language --------------------------------------------------------------------------

    /** Decodes the %XX runs as UTF-8 (and leaves a "+" alone, unlike URLDecoder). */
    private static String percentDecode(String text) {
        ByteArrayOutputStream out = new ByteArrayOutputStream();
        byte[] bytes = text.getBytes(StandardCharsets.UTF_8);
        for (int i = 0; i < bytes.length; i++) {
            if (bytes[i] == '%' && i + 2 < bytes.length) {
                out.write(Integer.parseInt(new String(bytes, i + 1, 2, StandardCharsets.US_ASCII), 16));
                i += 2;
            } else {
                out.write(bytes[i]);
            }
        }
        return new String(out.toByteArray(), StandardCharsets.UTF_8);
    }

    /** Literal parts are percent-decoded; {p}, {lang} and $name.field are replaced afterwards, untouched. */
    private static String expand(String value, Map<String, Map<String, String>> variables) {
        StringBuilder out = new StringBuilder();
        int last = 0;
        Matcher m = VARIABLE.matcher(value);
        while (m.find()) {
            out.append(percentDecode(value.substring(last, m.start())));
            if (m.group().equals("{p}")) {
                out.append(ARGS.get("prefix"));
            } else if (m.group().equals("{lang}")) {
                out.append(ARGS.get("lang"));
            } else {
                Map<String, String> v = variables.getOrDefault(m.group(1), Collections.<String, String>emptyMap());
                out.append(v.getOrDefault(m.group(2), ""));
            }
            last = m.end();
        }
        return out.append(percentDecode(value.substring(last))).toString();
    }

    // ---- helpers of the operations ----------------------------------------------------------------------

    private static Integer num(Map<String, String> p, String key) {
        return p.containsKey(key) ? Integer.valueOf(p.get(key)) : null;
    }

    private static Long lng(Map<String, String> p, String key) {
        return p.containsKey(key) ? Long.valueOf(p.get(key)) : null;
    }

    private static Boolean flag(Map<String, String> p, String key) {
        return p.containsKey(key) ? Boolean.valueOf(p.get(key)) : null;
    }

    private static List<String> items(String value) {
        List<String> out = new ArrayList<>();
        for (String s : value.split(",")) {
            if (!s.isEmpty()) {
                out.add(s);
            }
        }
        return out;
    }

    private static Map<String, String> tags(String value) {
        Map<String, String> out = new LinkedHashMap<>();
        for (String t : value.split(",")) {
            int i = t.indexOf(':');
            out.put(t.substring(0, i), t.substring(i + 1));
        }
        return out;
    }

    private static String text(Object value) {
        return value == null ? "" : String.valueOf(value);
    }

    private static String render(Message message) {
        String type = message.getContentType().split(";")[0].trim().toLowerCase();
        if (type.equals("application/octet-stream")) {
            return "b64:" + Base64.getEncoder().encodeToString(message.getBody());
        }
        return message.getBodyText();
    }

    private static Map<String, String> fields(String... pairs) {
        Map<String, String> out = new HashMap<>();
        for (int i = 0; i < pairs.length; i += 2) {
            out.put(pairs[i], pairs[i + 1]);
        }
        return out;
    }

    private static Map<String, String> topicFields(Topic t) {
        return fields("id", t.getId(), "name", t.getName(), "type", t.getType(), "fifo", text(t.getFifo()),
                "volatile", text(t.isVolatile()), "raw", t.getRaw());
    }

    private static Map<String, String> publishFields(PublishResponse r) {
        return fields("id", text(r.getId()), "duplicate", text(r.getDuplicate()), "matched", text(r.getMatched()));
    }

    private static ConsumeOptions consumeOptions(Map<String, String> p) {
        return new ConsumeOptions().maxMessages(num(p, "max")).waitTime(num(p, "wait")).visibilityTimeout(num(p, "visibility"));
    }

    private static PublishOptions publishOptions(Map<String, String> p) {
        PublishOptions options = new PublishOptions().groupId(p.get("groupId")).deduplicationId(p.get("deduplicationId"));
        return options.fifo("true".equals(p.get("fifo")));
    }

    private static Map<String, String> messageFields(List<Message> messages) {
        Map<String, String> out = fields("count", String.valueOf(messages.size()), "bodies",
                messages.stream().map(ContractDriver::render).collect(Collectors.joining(",")));
        if (!messages.isEmpty()) {
            Message first = messages.get(0);
            out.put("id", first.getId());
            out.put("receiptToken", first.getReceiptToken());
            out.put("deliveryCount", String.valueOf(first.getDeliveryCount()));
            out.put("contentType", first.getContentType());
            out.put("body", render(first));
        }
        return out;
    }

    // ---- the operations ---------------------------------------------------------------------------------

    private static Map<String, Operation> operations() {
        Map<String, Operation> ops = new HashMap<>();
        ops.put("createTopic", (c, p) -> {
            TopicRequest request = new TopicRequest().name(p.get("name")).type(p.get("type")).fifo(flag(p, "fifo"))
                    .volatileTopic(flag(p, "volatile")).ttl(lng(p, "ttl")).maxDeliveries(lng(p, "maxDeliveries"))
                    .maxMessageBytes(lng(p, "maxMessageBytes")).idleTimeout(lng(p, "idleTimeout"));
            if (p.containsKey("prefixes")) {
                request.prefixes(items(p.get("prefixes")));
            }
            if (p.containsKey("tags")) {
                request.tags(tags(p.get("tags")));
            }
            return topicFields(c.createTopic(request));
        });
        ops.put("getTopic", (c, p) -> topicFields(c.getTopic(p.get("name"))));
        ops.put("listTopics", (c, p) -> {
            List<Topic> topics = c.listAllTopics(p.get("name"));
            List<String> names = topics.stream().map(Topic::getName).sorted().collect(Collectors.toList());
            return fields("count", String.valueOf(topics.size()), "names", String.join(",", names));
        });
        ops.put("updateTopic", (c, p) -> {
            TopicUpdate update = new TopicUpdate();
            if (p.containsKey("prefixes")) {
                update.prefixes(items(p.get("prefixes")));
            }
            if (p.containsKey("tags")) {
                update.tags(tags(p.get("tags")));
            }
            if (p.containsKey("idleTimeout")) {
                update.idleTimeout(Long.parseLong(p.get("idleTimeout")));
            }
            // "null" goes back to the server default; absent leaves the field out.
            if (p.containsKey("ttl")) {
                if (p.get("ttl").equals("null")) {
                    update.ttlDefault();
                } else {
                    update.ttl(Long.parseLong(p.get("ttl")));
                }
            }
            if (p.containsKey("maxDeliveries")) {
                if (p.get("maxDeliveries").equals("null")) {
                    update.maxDeliveriesDefault();
                } else {
                    update.maxDeliveries(Long.parseLong(p.get("maxDeliveries")));
                }
            }
            if (p.containsKey("maxMessageBytes")) {
                if (p.get("maxMessageBytes").equals("null")) {
                    update.maxMessageBytesDefault();
                } else {
                    update.maxMessageBytes(Long.parseLong(p.get("maxMessageBytes")));
                }
            }
            return topicFields(c.updateTopic(p.get("name"), update));
        });
        ops.put("deleteTopic", (c, p) -> {
            c.deleteTopic(p.get("name"));
            return new HashMap<>();
        });

        ops.put("publishText", (c, p) -> publishFields(c.publish(p.get("topic"), p.get("body").getBytes(StandardCharsets.UTF_8),
                publishOptions(p).contentType("text/plain; charset=utf-8"))));
        ops.put("publishJson", (c, p) -> publishFields(c.publish(p.get("topic"), p.get("body").getBytes(StandardCharsets.UTF_8),
                publishOptions(p).contentType("application/json"))));
        ops.put("publishBytes", (c, p) -> publishFields(c.publish(p.get("topic"), Base64.getDecoder().decode(p.get("base64")),
                publishOptions(p).contentType(p.getOrDefault("contentType", "application/octet-stream")))));

        ops.put("consume", (c, p) -> messageFields(c.consume(p.get("topic"), consumeOptions(p))));
        ops.put("ack", (c, p) -> {
            c.ack(p.get("topic"), p.get("id"), p.get("receipt"));
            return new HashMap<>();
        });
        ops.put("changeVisibility", (c, p) -> {
            io.github.rafaelpontani.taba.VisibilityResponse r =
                    c.changeVisibility(p.get("topic"), p.get("id"), p.get("receipt"), Integer.parseInt(p.get("seconds")));
            return fields("id", r.getId(), "invisibleUntil", r.getInvisibleUntil());
        });

        // Consuming with a handler: mode ok (deleted), fail (throws), keep (delete = false) or slow (holds the
        // message for `hold` seconds, which only works if the invisibility is renewed).
        ops.put("handle", (c, p) -> {
            List<String> seen = Collections.synchronizedList(new ArrayList<String>());
            AtomicBoolean leaseLost = new AtomicBoolean();
            boolean failed = false;
            int handled = 0;
            try {
                handled = c.consume(p.get("topic"), consumeOptions(p), context -> {
                    seen.add(render(context.getMessage()));
                    if (p.get("mode").equals("fail")) {
                        throw new IllegalStateException("the handler failed");
                    }
                    if (p.get("mode").equals("keep")) {
                        context.setDelete(false);
                    }
                    if (p.get("mode").equals("slow")) {
                        Thread.sleep((long) (Double.parseDouble(p.get("hold")) * 1000));
                    }
                    leaseLost.set(context.isLeaseLost());
                });
            } catch (TabaException e) {
                throw e;
            } catch (RuntimeException e) {
                failed = true;
            }
            return fields("handled", String.valueOf(handled), "failed", String.valueOf(failed),
                    "bodies", String.join(",", seen), "leaseLost", String.valueOf(leaseLost.get()));
        });

        // The consume loop: stops after `count` messages or after `timeout` seconds. failFirst makes the first fail.
        ops.put("subscribe", (c, p) -> {
            List<String> seen = Collections.synchronizedList(new ArrayList<String>());
            CountDownLatch done = new CountDownLatch(1);
            try (Subscription subscription = c.subscribe(p.get("topic"),
                    new ConsumeOptions().visibilityTimeout(num(p, "visibility")), message -> {
                        seen.add(render(message));
                        if (seen.size() >= Integer.parseInt(p.get("count"))) {
                            done.countDown();
                        }
                        if ("true".equals(p.get("failFirst")) && seen.size() == 1) {
                            throw new IllegalStateException("the handler failed");
                        }
                    })) {
                done.await((long) (Double.parseDouble(p.get("timeout")) * 1000), TimeUnit.MILLISECONDS);
            }
            return fields("count", String.valueOf(seen.size()), "bodies", String.join(",", seen));
        });

        // A long polling cancelled from outside must end at once, not at the end of the wait.
        ops.put("cancelConsume", (c, p) -> {
            long started = System.nanoTime();
            CompletableFuture<List<Message>> future = c.consumeAsync(p.get("topic"), new ConsumeOptions().waitTime(20));
            Thread timer = new Thread(() -> {
                try {
                    Thread.sleep((long) (Double.parseDouble(p.get("after")) * 1000));
                } catch (InterruptedException e) {
                    return;
                }
                future.cancel(true);
            });
            timer.setDaemon(true);
            timer.start();
            boolean cancelled = false;
            try {
                future.get();
            } catch (CancellationException e) {
                cancelled = true;
            }
            boolean fast = (System.nanoTime() - started) < 10_000_000_000L;
            return fields("cancelled", String.valueOf(cancelled), "fast", String.valueOf(fast));
        });

        // Consumes and acknowledges until `count` messages came or `timeout` seconds passed. The bodies come sorted.
        ops.put("drain", (c, p) -> {
            List<String> seen = new ArrayList<>();
            long deadline = System.currentTimeMillis() + (long) (Double.parseDouble(p.get("timeout")) * 1000);
            while (seen.size() < Integer.parseInt(p.get("count")) && System.currentTimeMillis() < deadline) {
                for (Message message : c.consume(p.get("topic"),
                        new ConsumeOptions().maxMessages(10).waitTime(1).visibilityTimeout(60))) {
                    seen.add(render(message));
                    c.ack(p.get("topic"), message);
                }
            }
            Collections.sort(seen);
            return fields("count", String.valueOf(seen.size()), "bodies", String.join(",", seen));
        });
        ops.put("sleep", (c, p) -> {
            Thread.sleep((long) (Double.parseDouble(p.get("seconds")) * 1000));
            return new HashMap<>();
        });
        return ops;
    }

    // ---- the run ----------------------------------------------------------------------------------------

    private static List<String> check(Map<String, String> result, List<Expect> expects) {
        List<String> problems = new ArrayList<>();
        boolean failure = result.containsKey("error");
        boolean expectsError = expects.stream().anyMatch(e -> e.key.equals("error"));
        if (failure && !expectsError) {
            problems.add("unexpected error " + result.get("error") + " (status " + result.get("status") + "): " + result.get("message"));
        }
        if (!failure && expectsError) {
            problems.add("expected an error, but the call succeeded");
        }
        for (Expect e : expects) {
            String actual = result.get(e.key);
            boolean mismatch;
            if (e.op == '~') {
                mismatch = actual == null || !actual.contains(e.value);
            } else if (e.value.equals("*")) {
                mismatch = actual == null || actual.isEmpty();
            } else {
                mismatch = !e.value.equals(actual);
            }
            if (mismatch) {
                problems.add(e.key + " " + e.op + " \"" + e.value + "\", got \"" + actual + "\"");
            }
        }
        return problems;
    }

    public static void main(String[] argv) throws Exception {
        for (int i = 0; i < argv.length; i += 2) {
            String key = argv[i].substring(2);
            if (key.equals("scenario")) {
                SCENARIOS.add(argv[i + 1]);
            } else {
                ARGS.put(key, argv[i + 1]);
            }
        }
        Map<String, Operation> operations = operations();
        Map<String, TabaClient> clients = new HashMap<>();
        Map<String, Map<String, String>> variables = new HashMap<>();
        int passed = 0;
        int failed = 0;
        int skipped = 0;
        for (String path : SCENARIOS) {
            List<String> lines = Files.readAllLines(Paths.get(path), StandardCharsets.UTF_8);
            for (int n = 0; n < lines.size(); n++) {
                String text = lines.get(n).trim();
                if (text.isEmpty() || text.startsWith("#")) {
                    continue;
                }
                String where = Paths.get(path).getFileName() + ":" + (n + 1);
                int arrow = text.indexOf(" => ");
                String[] head = (arrow < 0 ? text : text.substring(0, arrow)).trim().split("\\s+");
                Map<String, String> params = new HashMap<>();
                for (int i = 1; i < head.length; i++) {
                    int eq = head[i].indexOf('=');
                    params.put(head[i].substring(0, eq), expand(head[i].substring(eq + 1), variables));
                }
                List<Expect> expects = new ArrayList<>();
                if (arrow >= 0) {
                    for (String token : text.substring(arrow + 4).trim().split("\\s+")) {
                        if (token.isEmpty()) {
                            continue;
                        }
                        int at = 0;
                        while (token.charAt(at) != '=' && token.charAt(at) != '~') {
                            at++;
                        }
                        expects.add(new Expect(token.substring(0, at), token.charAt(at), expand(token.substring(at + 1), variables)));
                    }
                }
                Operation operation = operations.get(head[0]);
                if (operation == null) {
                    skipped++;
                    System.out.println("SKIP " + where + " " + head[0] + " (not supported by this connector)");
                    continue;
                }
                String clientKey = params.get("url") + "|" + params.get("token") + "|" + params.get("attempts");
                TabaClient client = clients.get(clientKey);
                if (client == null) {
                    TabaConfig config = TabaConfig.of(params.getOrDefault("url", ARGS.get("url")))
                            .token(params.getOrDefault("token", ARGS.get("token")));
                    if (params.containsKey("attempts")) {
                        config.maxAttempts(Integer.parseInt(params.get("attempts")));
                    }
                    client = new TabaClient(config);
                    clients.put(clientKey, client);
                }
                Map<String, String> result;
                try {
                    result = operation.run(client, params);
                } catch (TabaException e) {
                    result = fields("error", e.getCode(), "status", String.valueOf(e.getStatus()), "message", e.getMessage());
                }
                if (params.containsKey("as")) {
                    variables.put(params.get("as"), result);
                }
                List<String> problems = check(result, expects);
                if (problems.isEmpty()) {
                    passed++;
                    System.out.println("PASS " + where + " " + head[0]);
                } else {
                    failed++;
                    System.out.println("FAIL " + where + " " + text);
                    for (String problem : problems) {
                        System.out.println("       " + problem);
                    }
                }
            }
        }
        for (TabaClient client : clients.values()) {
            client.close();
        }
        System.out.println(ARGS.get("lang") + ": " + passed + " passed, " + failed + " failed, " + skipped + " skipped");
        System.exit(failed == 0 ? 0 : 1);
    }
}
