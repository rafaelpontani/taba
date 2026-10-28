package io.github.rafaelpontani.taba;

import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.Base64;
import java.util.List;

/** Reads the API's JSON responses and builds the models. */
final class ModelParser {
    private ModelParser() {
    }

    private static TabaException invalid(String what, Json.JsonException e) {
        return TabaException.client("client.invalid-response", what + ": " + e.getMessage());
    }

    private static Json.Fields fields(String raw, String what) {
        try {
            return Json.Fields.parse(raw);
        } catch (Json.JsonException e) {
            throw invalid(what, e);
        }
    }

    private static String orEmpty(String value) {
        return value == null ? "" : value;
    }

    static Topic parseTopic(String raw) {
        Json.Fields f = fields(raw, "topic");
        try {
            return new Topic(
                    orEmpty(f.string("id")),
                    orEmpty(f.string("name")),
                    orEmpty(f.string("type")),
                    orEmpty(f.string("createdAt")),
                    f.bool("fifo"),
                    f.bool("volatile"),
                    raw);
        } catch (Json.JsonException e) {
            throw invalid("topic", e);
        }
    }

    private static boolean isJsonType(String contentType) {
        String media = contentType.split(";")[0].trim().toLowerCase(java.util.Locale.ROOT);
        return media.equals("application/json") || media.endsWith("+json");
    }

    static Message parseMessage(String raw) {
        Json.Fields f = fields(raw, "message");
        try {
            String contentType = f.string("contentType");
            if (contentType == null) {
                contentType = "application/json";
            }
            String body = f.raw("body");
            byte[] bytes;
            if (body == null) {
                bytes = new byte[0];
            } else if ("base64".equals(f.string("bodyEncoding"))) {
                try {
                    String encoded = Json.decodeString(body);
                    // The JDK decoder accepts a missing padding; the contract does not.
                    if (encoded.length() % 4 != 0) {
                        throw new IllegalArgumentException("bad padding");
                    }
                    bytes = Base64.getDecoder().decode(encoded);
                } catch (IllegalArgumentException e) {
                    throw TabaException.client("client.invalid-response", "invalid base64 body");
                }
            } else if (isJsonType(contentType)) {
                bytes = body.getBytes(StandardCharsets.UTF_8);
            } else {
                bytes = Json.decodeString(body).getBytes(StandardCharsets.UTF_8);
            }
            Long deliveryCount = f.integer("deliveryCount");
            return new Message(
                    orEmpty(f.string("id")),
                    bytes,
                    contentType,
                    orEmpty(f.string("receiptToken")),
                    deliveryCount == null ? 0 : deliveryCount,
                    orEmpty(f.string("publishedAt")),
                    orEmpty(f.string("invisibleUntil")),
                    f.string("groupId"),
                    f.string("deduplicationId"),
                    f.string("deadLetteredAt"),
                    f.string("deadLetterReason"),
                    raw);
        } catch (Json.JsonException e) {
            throw invalid("message", e);
        }
    }

    static List<Message> parseMessages(String raw) {
        Json.Fields f = fields(raw, "consume");
        try {
            String list = f.raw("messages");
            List<Message> messages = new ArrayList<>();
            for (String item : Json.arrayItems(list == null ? "[]" : list)) {
                messages.add(parseMessage(item));
            }
            return messages;
        } catch (Json.JsonException e) {
            throw invalid("messages", e);
        }
    }

    static Page<Topic> parsePage(String raw) {
        Json.Fields f = fields(raw, "page");
        try {
            String list = f.raw("items");
            List<Topic> items = new ArrayList<>();
            for (String item : Json.arrayItems(list == null ? "[]" : list)) {
                items.add(parseTopic(item));
            }
            return new Page<>(items, f.string("nextCursor"), raw);
        } catch (Json.JsonException e) {
            throw invalid("page items", e);
        }
    }

    static PublishResponse parsePublishResponse(String raw) {
        Json.Fields f = fields(raw, "publication");
        try {
            List<DistributorDelivery> deliveries = new ArrayList<>();
            String list = f.raw("deliveries");
            if (list != null) {
                for (String item : Json.arrayItems(list)) {
                    Json.Fields d = Json.Fields.parse(item);
                    String error = d.raw("error");
                    Json.Fields e = error != null && error.startsWith("{") ? Json.Fields.parse(error) : null;
                    deliveries.add(new DistributorDelivery(
                            orEmpty(d.string("topic")),
                            d.string("id"),
                            e == null ? null : orEmpty(e.string("code")),
                            e == null ? null : e.string("message")));
                }
            }
            return new PublishResponse(f.string("id"), f.bool("duplicate"), f.integer("matched"), deliveries, raw);
        } catch (Json.JsonException e) {
            throw invalid("publication", e);
        }
    }

    static VisibilityResponse parseVisibilityResponse(String raw) {
        Json.Fields f = fields(raw, "visibility");
        try {
            return new VisibilityResponse(orEmpty(f.string("id")), orEmpty(f.string("invisibleUntil")), raw);
        } catch (Json.JsonException e) {
            throw invalid("visibility", e);
        }
    }
}
