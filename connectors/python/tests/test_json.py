import unittest

from taba._json import JsonError, JsonFields, array_items


class JsonReaderTests(unittest.TestCase):
    def test_the_reader_delivers_each_field_as_a_text_fragment_without_reserializing(self) -> None:
        f = JsonFields.parse(
            ' { "a" : 1.0 , "b":{"x":[1, 2,{"y":"}"}]} , "c":"q\\"uote\\u00e9\\ud83d\\ude00" , "d":null,"e":true } '
        )
        self.assertEqual(f.raw("a"), "1.0")
        self.assertEqual(f.raw("b"), '{"x":[1, 2,{"y":"}"}]}')
        self.assertEqual(f.string("c"), 'q"uoteé😀')
        self.assertIsNone(f.raw("d"))
        self.assertIsNone(f.raw("missing"))
        self.assertIs(f.boolean("e"), True)

    def test_large_numbers_and_deep_nesting_are_kept_and_do_not_overflow_the_stack(self) -> None:
        deep = "[" * 50000 + "]" * 50000
        f = JsonFields.parse('{"n":12345678901234567890123,"deep":%s}' % deep)
        self.assertEqual(f.raw("n"), "12345678901234567890123")
        self.assertEqual(len(f.raw("deep") or ""), 100000)

    def test_array_items_are_text_fragments_and_empty_values_are_accepted(self) -> None:
        self.assertEqual(array_items("[]"), [])
        self.assertEqual(
            array_items(' [ 1 , "a,b" , {"k":[1,2]} , [ ] , null ] '), ["1", '"a,b"', '{"k":[1,2]}', "[ ]", "null"]
        )
        self.assertIsNone(JsonFields.parse("{}").raw("x"))
        self.assertEqual(JsonFields.parse('{"e":""}').string("e"), "")

    def test_malformed_documents_are_a_json_error_and_not_a_crash(self) -> None:
        for bad in ["", "x", "[1]", "{", '{"a"}', '{"a":}', '{"a":1,}', '{"a":"x', '{"a":1} extra', '{"a":[1,2}']:
            with self.assertRaises(JsonError, msg=bad):
                JsonFields.parse(bad)
        with self.assertRaises(JsonError):
            array_items("{}")
        with self.assertRaises(JsonError):
            JsonFields.parse('{"a":"x"}').integer("a")
        with self.assertRaises(JsonError):
            JsonFields.parse('{"a":"1_0"}').integer("a")
