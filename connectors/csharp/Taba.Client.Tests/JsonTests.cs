using System;
using System.Linq;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary>O leitor de JSON mínimo (doc 19, seção 18.5): os mesmos casos do conector Rust.</summary>
    [TestFixture]
    public class JsonTests
    {
        /// UC: UC-CNT-021
        [Test]
        public void skips_every_kind_of_value()
        {
            foreach (string doc in new[]
            {
                "null", "true", "false", "0", "-12", "3.14", "1e10", "-2.5E-3", "\"x\"", "[]", "{}",
                "[1, [2, [3]]]", "{\"a\": {\"b\": [true, null]}}", " \n\t 7 ",
            })
            {
                int end = Json.SkipValue(doc, 0);
                Assert.That(doc.Substring(end).Trim(), Is.Empty, doc);
                Assert.DoesNotThrow(() => Json.Validate(doc), doc);
            }
        }

        /// UC: UC-CNT-021
        [Test]
        public void rejects_malformed_documents()
        {
            foreach (string doc in new[]
            {
                "", "{", "[1,", "{\"a\"}", "{\"a\":}", "tru", "01", "1.", "-", "\"abc", "\"\\x\"", "\"\\u12G4\"",
                "[1 2]", "{\"a\":1 \"b\":2}", "\"a\nb\"", "{} x", "nul", "[1,]",
            })
            {
                Assert.Throws<JsonException>(() => Json.Validate(doc), "deveria recusar " + doc);
            }
        }

        /// UC: UC-CNT-021
        [Test]
        public void deep_nesting_is_bounded_not_a_stack_overflow()
        {
            string ok = new string('[', Json.MaxDepth - 1) + "1" + new string(']', Json.MaxDepth - 1);
            Assert.DoesNotThrow(() => Json.SkipValue(ok, 0));
            string deep = new string('[', 100000) + "1" + new string(']', 100000);
            Assert.Throws<JsonException>(() => Json.SkipValue(deep, 0));
        }

        /// UC: UC-CNT-021
        [Test]
        public void object_fields_give_the_raw_text_of_each_value()
        {
            string doc = "{ \"id\" : \"a\\nb\", \"body\" : { \"x\": [1, 2] } ,\"n\":12345678901234567890, \"e\":{} }\n";
            var fields = Json.ObjectFields(doc);
            Assert.That(fields.Select(f => f.Key), Is.EqualTo(new[] { "id", "body", "n", "e" }));
            Assert.That(fields[0].Value, Is.EqualTo("\"a\\nb\""));
            Assert.That(fields[1].Value, Is.EqualTo("{ \"x\": [1, 2] }"));
            Assert.That(fields[2].Value, Is.EqualTo("12345678901234567890"));
            Assert.That(fields[3].Value, Is.EqualTo("{}"));
            Assert.Throws<JsonException>(() => Json.ObjectFields("{} x"));
            Assert.Throws<JsonException>(() => Json.ObjectFields("[]"));
            Assert.That(Json.ObjectFields(" { } "), Is.Empty);
        }

        /// UC: UC-CNT-021
        [Test]
        public void decodes_escapes_unicode_and_surrogate_pairs()
        {
            Assert.That(Json.DecodeString("\"a\\\"b\\\\c\\/d\""), Is.EqualTo("a\"b\\c/d"));
            Assert.That(Json.DecodeString("\"\\b\\f\\n\\r\\t\""), Is.EqualTo("\b\f\n\r\t"));
            Assert.That(Json.DecodeString("\"\\u00e9\\u4e2d\""), Is.EqualTo("é中"));
            Assert.That(Json.DecodeString("\"\\ud83d\\ude00\""), Is.EqualTo("😀"));
            Assert.That(Json.DecodeString("\"😀 direto\""), Is.EqualTo("😀 direto"));
            // Substituto sozinho não derruba: vira o caractere de substituição.
            Assert.That(Json.DecodeString("\"\\ud83d x\""), Is.EqualTo("\uFFFD x"));
            Assert.That(Json.DecodeString("\"\\ude00\""), Is.EqualTo("\uFFFD"));
            Assert.Throws<JsonException>(() => Json.DecodeString("abc"));
        }

        /// UC: UC-CNT-021, UC-CNT-025
        [Test]
        public void quote_round_trips_through_decode()
        {
            foreach (string s in new[] { "", "simples", "aspas \" e barra \\", "linha\nnova\ttab", "\u0001ctrl", "çã 中 😀" })
            {
                Assert.That(Json.DecodeString(Json.Quote(s)), Is.EqualTo(s));
            }
            Assert.That(Json.Quote("a\u001f"), Is.EqualTo("\"a\\u001f\""));
        }

        /// UC: UC-CNT-021
        [Test]
        public void array_items_split_values_with_nested_commas()
        {
            var items = Json.ArrayItems(" [ \"a,b\", [1,2], {\"k\": [3,4]}, 5 ] ");
            Assert.That(items, Is.EqualTo(new[] { "\"a,b\"", "[1,2]", "{\"k\": [3,4]}", "5" }));
            Assert.That(Json.ArrayItems("[]"), Is.Empty);
            Assert.Throws<JsonException>(() => Json.ArrayItems("[1,]"));
        }

        /// UC: UC-CNT-021, UC-CNT-028
        [Test]
        public void fields_look_up_typed_values_and_ignore_unknown_ones()
        {
            var f = JsonFields.Parse("{\"s\":\"x\",\"n\":42,\"neg\":-1,\"t\":true,\"f\":false,\"nul\":null,\"obj\":{\"a\":1}}");
            Assert.That(f.String("s"), Is.EqualTo("x"));
            Assert.That(f.Integer("n"), Is.EqualTo(42));
            Assert.That(f.Integer("neg"), Is.Null);
            Assert.That(f.Boolean("t"), Is.True);
            Assert.That(f.Boolean("f"), Is.False);
            Assert.That(f.String("n"), Is.Null);
            Assert.That(f.String("nul"), Is.Null);
            Assert.That(f.Raw("obj"), Is.EqualTo("{\"a\":1}"));
            Assert.That(f.Raw("ausente"), Is.Null);
        }
    }
}
