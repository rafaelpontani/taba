using System;
using System.Collections.Generic;

namespace Taba
{
    /// <summary>Reads the API's JSON responses and builds the <c>Taba.Client.Abstractions</c> models.</summary>
    internal static class ModelParser
    {
        public static JsonFields Fields(string raw, string what)
        {
            try
            {
                return JsonFields.Parse(raw);
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", what + ": " + e.Message);
            }
        }

        public static Topic ParseTopic(string raw)
        {
            JsonFields f = Fields(raw, "topic");
            return new Topic
            {
                Id = f.String("id") ?? string.Empty,
                Name = f.String("name") ?? string.Empty,
                Type = f.String("type") ?? string.Empty,
                CreatedAt = f.String("createdAt") ?? string.Empty,
                Fifo = f.Boolean("fifo"),
                Volatile = f.Boolean("volatile"),
                Raw = raw,
            };
        }

        public static Message ParseMessage(string raw)
        {
            JsonFields f = Fields(raw, "message");
            string contentType = f.String("contentType") ?? "application/json";
            string body = f.Raw("body");
            byte[] bytes;
            try
            {
                if (body == null)
                {
                    bytes = new byte[0];
                }
                else if (f.String("bodyEncoding") == "base64")
                {
                    bytes = Convert.FromBase64String(Json.DecodeString(body));
                }
                else if (IsJsonType(contentType))
                {
                    bytes = Utf8.Encoding.GetBytes(body);
                }
                else
                {
                    bytes = Utf8.Encoding.GetBytes(Json.DecodeString(body));
                }
            }
            catch (FormatException)
            {
                throw TabaException.Client("client.invalid-response", "invalid base64 body");
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "message body: " + e.Message);
            }
            return new Message
            {
                Id = f.String("id") ?? string.Empty,
                Body = bytes,
                ContentType = contentType,
                ReceiptToken = f.String("receiptToken") ?? string.Empty,
                DeliveryCount = f.Integer("deliveryCount") ?? 0,
                PublishedAt = f.String("publishedAt") ?? string.Empty,
                InvisibleUntil = f.String("invisibleUntil") ?? string.Empty,
                GroupId = f.String("groupId"),
                DeduplicationId = f.String("deduplicationId"),
                DeadLetteredAt = f.String("deadLetteredAt"),
                DeadLetterReason = f.String("deadLetterReason"),
                Raw = raw,
            };
        }

        public static PublishResponse ParsePublishResponse(string raw)
        {
            JsonFields f = Fields(raw, "publication");
            var deliveries = new List<DistributorDelivery>();
            try
            {
                string list = f.Raw("deliveries");
                if (list != null)
                {
                    foreach (string item in Json.ArrayItems(list))
                    {
                        JsonFields d = JsonFields.Parse(item);
                        string error = d.Raw("error");
                        JsonFields e = error != null && error.StartsWith("{", StringComparison.Ordinal)
                            ? JsonFields.Parse(error)
                            : null;
                        deliveries.Add(new DistributorDelivery(
                            d.String("topic") ?? string.Empty,
                            d.String("id"),
                            e == null ? null : e.String("code") ?? string.Empty,
                            e == null ? null : e.String("message")));
                    }
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "entregas do distribuidor: " + e.Message);
            }
            return new PublishResponse
            {
                Id = f.String("id"),
                Duplicate = f.Boolean("duplicate"),
                Matched = f.Integer("matched"),
                Deliveries = deliveries,
                Raw = raw,
            };
        }

        /// <summary>The <c>results</c> of a batch, one per item in the order of the request.</summary>
        public static IList<BatchResult> ParseBatchResults(string raw)
        {
            JsonFields f = Fields(raw, "lote");
            var results = new List<BatchResult>();
            try
            {
                foreach (string item in Json.ArrayItems(f.Raw("results") ?? "[]"))
                {
                    JsonFields r = JsonFields.Parse(item);
                    string error = r.Raw("error");
                    JsonFields e = error != null && error.StartsWith("{", StringComparison.Ordinal)
                        ? JsonFields.Parse(error)
                        : null;
                    results.Add(new BatchResult
                    {
                        Status = (int)(r.Integer("status") ?? 0),
                        Id = r.String("id"),
                        Duplicate = r.Boolean("duplicate"),
                        Matched = r.Integer("matched"),
                        InvisibleUntil = r.String("invisibleUntil"),
                        ErrorCode = e == null ? null : e.String("code") ?? string.Empty,
                        ErrorMessage = e == null ? null : e.String("message"),
                        Raw = item,
                    });
                }
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "resultados do lote: " + e.Message);
            }
            return results;
        }

        public static VisibilityResponse ParseVisibilityResponse(string raw)
        {
            JsonFields f = Fields(raw, "invisibilidade");
            return new VisibilityResponse
            {
                Id = f.String("id") ?? string.Empty,
                InvisibleUntil = f.String("invisibleUntil") ?? string.Empty,
                Raw = raw,
            };
        }

        private static bool IsJsonType(string contentType)
        {
            string media = contentType.Split(';')[0].Trim().ToLowerInvariant();
            return media == "application/json" || media.EndsWith("+json", StringComparison.Ordinal);
        }
    }
}
