using System.Collections.Generic;
using System.Globalization;

namespace Taba
{
    internal static class JsonWriter
    {
        public static string Map(IDictionary<string, string> pairs)
        {
            var items = new List<string>();
            foreach (var pair in pairs)
            {
                items.Add(Json.Quote(pair.Key) + ":" + Json.Quote(pair.Value));
            }
            return "{" + string.Join(",", items.ToArray()) + "}";
        }

        public static string List(IEnumerable<string> values)
        {
            var items = new List<string>();
            foreach (string value in values)
            {
                items.Add(Json.Quote(value));
            }
            return "[" + string.Join(",", items.ToArray()) + "]";
        }

        public static string Bool(bool value)
        {
            return value ? "true" : "false";
        }

        public static string Number(long value)
        {
            return value.ToString(CultureInfo.InvariantCulture);
        }
    }
}
