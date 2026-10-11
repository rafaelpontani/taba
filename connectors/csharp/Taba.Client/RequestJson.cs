using System.Collections.Generic;

namespace Taba
{
    /// <summary>Monta o corpo JSON das requisições <c>POST /topics</c> e <c>PATCH /topics/{topic}</c>.</summary>
    internal static class RequestJson
    {
        public static string Create(TopicRequest request)
        {
            var fields = new List<string>
            {
                "\"name\":" + Json.Quote(request.Name ?? string.Empty),
                "\"type\":" + Json.Quote(request.Type ?? string.Empty),
            };
            if (request.Tags != null && request.Tags.Count > 0)
            {
                fields.Add("\"tags\":" + JsonWriter.Map(request.Tags));
            }
            if (request.Prefixes != null && request.Prefixes.Count > 0)
            {
                fields.Add("\"prefixes\":" + JsonWriter.List(request.Prefixes));
            }
            if (request.Volatile.HasValue)
            {
                fields.Add("\"volatile\":" + JsonWriter.Bool(request.Volatile.Value));
            }
            if (request.IdleTimeout.HasValue)
            {
                fields.Add("\"idleTimeout\":" + JsonWriter.Number(request.IdleTimeout.Value));
            }
            if (request.Fifo.HasValue)
            {
                fields.Add("\"fifo\":" + JsonWriter.Bool(request.Fifo.Value));
            }
            if (request.MaxMessageBytes.HasValue)
            {
                fields.Add("\"maxMessageBytes\":" + JsonWriter.Number(request.MaxMessageBytes.Value));
            }
            if (request.Ttl.HasValue)
            {
                fields.Add("\"ttl\":" + JsonWriter.Number(request.Ttl.Value));
            }
            if (request.MaxDeliveries.HasValue)
            {
                fields.Add("\"maxDeliveries\":" + JsonWriter.Number(request.MaxDeliveries.Value));
            }
            return "{" + string.Join(",", fields.ToArray()) + "}";
        }

        public static string Update(TopicUpdate update)
        {
            var fields = new List<string>();
            if (update.Tags != null)
            {
                fields.Add("\"tags\":" + JsonWriter.Map(update.Tags));
            }
            if (update.Prefixes != null)
            {
                fields.Add("\"prefixes\":" + JsonWriter.List(update.Prefixes));
            }
            if (update.IdleTimeout.HasValue)
            {
                fields.Add("\"idleTimeout\":" + JsonWriter.Number(update.IdleTimeout.Value));
            }
            update.MaxMessageBytes.Push("maxMessageBytes", fields);
            update.Ttl.Push("ttl", fields);
            update.MaxDeliveries.Push("maxDeliveries", fields);
            return "{" + string.Join(",", fields.ToArray()) + "}";
        }
    }
}
