package io.github.rafaelpontani.taba;

import static org.junit.jupiter.api.Assertions.assertEquals;
import static org.junit.jupiter.api.Assertions.assertNull;
import static org.junit.jupiter.api.Assertions.assertThrows;

import java.util.Arrays;
import java.util.Collections;
import org.junit.jupiter.api.Test;

class JsonTest {
    @Test
    void theReaderDeliversEachFieldAsATextFragmentWithoutReserializing() {
        Json.Fields f = Json.Fields.parse(
                " { \"a\" : 1.0 , \"b\":{\"x\":[1, 2,{\"y\":\"}\"}]} , \"c\":\"q\\\"uote\\u00e9\\ud83d\\ude00\" , \"d\":null,\"e\":true } ");
        assertEquals("1.0", f.raw("a"));
        assertEquals("{\"x\":[1, 2,{\"y\":\"}\"}]}", f.raw("b"));
        assertEquals("q\"uoteé\uD83D\uDE00", f.string("c"));
        assertNull(f.raw("d"));
        assertNull(f.raw("missing"));
        assertEquals(Boolean.TRUE, f.bool("e"));
    }

    @Test
    void largeNumbersAndDeepNestingAreKeptAndDoNotOverflowTheStack() {
        String deep = "[".repeat(50000) + "]".repeat(50000);
        Json.Fields f = Json.Fields.parse("{\"n\":12345678901234567890123,\"deep\":" + deep + "}");
        assertEquals("12345678901234567890123", f.raw("n"));
        assertEquals(100000, f.raw("deep").length());
    }

    @Test
    void arrayItemsAreTextFragmentsAndEmptyValuesAreAccepted() {
        assertEquals(Collections.emptyList(), Json.arrayItems("[]"));
        assertEquals(
                Arrays.asList("1", "\"a,b\"", "{\"k\":[1,2]}", "[ ]", "null"),
                Json.arrayItems(" [ 1 , \"a,b\" , {\"k\":[1,2]} , [ ] , null ] "));
        assertNull(Json.Fields.parse("{}").raw("x"));
        assertEquals("", Json.Fields.parse("{\"e\":\"\"}").string("e"));
    }

    @Test
    void quoteEscapesWhatJsonRequires() {
        assertEquals("\"a\\\"b\\\\c\\n\\u0001é\"", Json.quote("a\"b\\c\n\u0001é"));
        assertEquals("a\"b\\c\n\u0001é", Json.decodeString(Json.quote("a\"b\\c\n\u0001é")));
    }

    @Test
    void malformedDocumentsAreAJsonExceptionAndNotACrash() {
        for (String bad : new String[] {"", "x", "[1]", "{", "{\"a\"}", "{\"a\":}", "{\"a\":1,}", "{\"a\":\"x", "{\"a\":1} extra", "{\"a\":[1,2}"}) {
            assertThrows(Json.JsonException.class, () -> Json.Fields.parse(bad), bad);
        }
        assertThrows(Json.JsonException.class, () -> Json.arrayItems("{}"));
        assertThrows(Json.JsonException.class, () -> Json.Fields.parse("{\"a\":\"x\"}").integer("a"));
        assertThrows(Json.JsonException.class, () -> Json.decodeString("\"\\u12\""));
    }
}
