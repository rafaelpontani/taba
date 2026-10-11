using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;

namespace Taba
{
    internal static class Utf8
    {
        /// <summary>UTF-8 sem marca de ordem de bytes: o texto vai e volta sempre assim, qualquer que seja o charset local.</summary>
        public static readonly Encoding Encoding = new UTF8Encoding(false);
    }

    /// <summary>Um tópico (fila ou distribuidor).</summary>
    public sealed class Topic
    {
        internal Topic()
        {
        }

        public string Id { get; private set; }

        public string Name { get; private set; }

        /// <summary><c>queue</c> ou <c>distributor</c>.</summary>
        public string Type { get; private set; }

        /// <summary>Data de criação, como texto RFC 3339.</summary>
        public string CreatedAt { get; private set; }

        public bool? Fifo { get; private set; }

        public bool? Volatile { get; private set; }

        /// <summary>O JSON do tópico, como o servidor devolveu.</summary>
        public string Raw { get; private set; }

        internal static Topic Parse(string raw)
        {
            JsonFields f = Fields(raw, "tópico");
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

        internal static JsonFields Fields(string raw, string what)
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
    }

    /// <summary>Uma página (<c>Page&lt;T&gt;</c>): os itens e o cursor da próxima, se houver.</summary>
    public sealed class Page<T>
    {
        internal Page(IList<T> items, string nextCursor, string raw)
        {
            Items = items;
            NextCursor = nextCursor;
            Raw = raw;
        }

        public IList<T> Items { get; private set; }

        /// <summary>O cursor da próxima página, ou <c>null</c> na última.</summary>
        public string NextCursor { get; private set; }

        public string Raw { get; private set; }
    }

    /// <summary>Uma mensagem entregue. O <see cref="Body"/> sai sempre como bytes (doc 19, seção 18.4).</summary>
    public sealed class Message
    {
        internal Message()
        {
        }

        public string Id { get; private set; }

        /// <summary>
        /// JSON: o texto JSON tal qual veio. <c>text/*</c> em UTF-8: o texto em UTF-8. Base64: os bytes
        /// já decodificados.
        /// </summary>
        public byte[] Body { get; private set; }

        public string ContentType { get; private set; }

        public string ReceiptToken { get; private set; }

        public long DeliveryCount { get; private set; }

        /// <summary>Data de publicação, como texto RFC 3339.</summary>
        public string PublishedAt { get; private set; }

        /// <summary>Até quando a mensagem fica invisível, como texto RFC 3339.</summary>
        public string InvisibleUntil { get; private set; }

        public string GroupId { get; private set; }

        public string DeduplicationId { get; private set; }

        public string DeadLetteredAt { get; private set; }

        public string DeadLetterReason { get; private set; }

        /// <summary>O JSON da mensagem, como o servidor devolveu.</summary>
        public string Raw { get; private set; }

        /// <summary>O <see cref="Body"/> como texto UTF-8.</summary>
        public string BodyText
        {
            get { return Utf8.Encoding.GetString(Body); }
        }

        private static bool IsJsonType(string contentType)
        {
            string media = contentType.Split(';')[0].Trim().ToLowerInvariant();
            return media == "application/json" || media.EndsWith("+json", StringComparison.Ordinal);
        }

        internal static Message Parse(string raw)
        {
            JsonFields f = Topic.Fields(raw, "mensagem");
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
                throw TabaException.Client("client.invalid-response", "corpo base64 inválido");
            }
            catch (JsonException e)
            {
                throw TabaException.Client("client.invalid-response", "corpo da mensagem: " + e.Message);
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
    }

    /// <summary>A cópia de uma publicação feita por um distribuidor para uma fila.</summary>
    public sealed class DistributorDelivery
    {
        internal DistributorDelivery(string topic, string id, string errorCode, string errorMessage)
        {
            Topic = topic;
            Id = id;
            ErrorCode = errorCode;
            ErrorMessage = errorMessage;
        }

        /// <summary>A fila de destino.</summary>
        public string Topic { get; private set; }

        /// <summary>O <c>id</c> da cópia, quando foi criada.</summary>
        public string Id { get; private set; }

        /// <summary>O <c>code</c> do erro, quando a cópia falhou (falha parcial não é erro do distribuidor).</summary>
        public string ErrorCode { get; private set; }

        public string ErrorMessage { get; private set; }

        public bool Succeeded
        {
            get { return ErrorCode == null; }
        }
    }

    /// <summary>A resposta de uma publicação: <c>PublishResponse</c> (fila) ou <c>DistributorPublishResponse</c>.</summary>
    public sealed class PublishResponse
    {
        internal PublishResponse()
        {
        }

        /// <summary>O <c>id</c> da mensagem (fila). Ausente na resposta de um distribuidor.</summary>
        public string Id { get; private set; }

        /// <summary>Só na fila FIFO: <c>true</c> quando repetiu uma publicação da janela de deduplicação.</summary>
        public bool? Duplicate { get; private set; }

        /// <summary>Só no distribuidor: quantas filas casaram.</summary>
        public long? Matched { get; private set; }

        /// <summary>Só no distribuidor: uma entrada por fila que casou.</summary>
        public IList<DistributorDelivery> Deliveries { get; private set; }

        public string Raw { get; private set; }

        internal static PublishResponse Parse(string raw)
        {
            JsonFields f = Topic.Fields(raw, "publicação");
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
    }

    /// <summary>A resposta de <c>changeVisibility</c>.</summary>
    public sealed class VisibilityResponse
    {
        internal VisibilityResponse()
        {
        }

        public string Id { get; private set; }

        public string InvisibleUntil { get; private set; }

        public string Raw { get; private set; }

        internal static VisibilityResponse Parse(string raw)
        {
            JsonFields f = Topic.Fields(raw, "invisibilidade");
            return new VisibilityResponse
            {
                Id = f.String("id") ?? string.Empty,
                InvisibleUntil = f.String("invisibleUntil") ?? string.Empty,
                Raw = raw,
            };
        }
    }

    /// <summary>Um campo do <c>PATCH</c> que aceita voltar ao padrão do servidor (<c>null</c>).</summary>
    public struct Field<T> where T : struct, IFormattable
    {
        private readonly int _state;
        private readonly T _value;

        private Field(int state, T value)
        {
            _state = state;
            _value = value;
        }

        /// <summary>Não envia o campo (o valor inicial).</summary>
        public static Field<T> Keep
        {
            get { return default(Field<T>); }
        }

        /// <summary>Envia <c>null</c>: volta ao padrão do servidor.</summary>
        public static Field<T> UseDefault
        {
            get { return new Field<T>(1, default(T)); }
        }

        public static Field<T> Set(T value)
        {
            return new Field<T>(2, value);
        }

        public static implicit operator Field<T>(T value)
        {
            return Set(value);
        }

        internal void Push(string name, List<string> fields)
        {
            if (_state == 1)
            {
                fields.Add("\"" + name + "\":null");
            }
            else if (_state == 2)
            {
                fields.Add("\"" + name + "\":" + _value.ToString(null, CultureInfo.InvariantCulture));
            }
        }
    }

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

    /// <summary>
    /// <c>POST /topics</c>. O conector manda só o que foi preenchido; o que não vale para o tipo, o
    /// servidor recusa.
    /// </summary>
    public sealed class TopicRequest
    {
        public string Name { get; set; }

        /// <summary><c>queue</c> ou <c>distributor</c>.</summary>
        public string Type { get; set; }

        public IDictionary<string, string> Tags { get; set; }

        /// <summary>Só no distribuidor: os prefixos de nome das filas para onde cada publicação é copiada.</summary>
        public IList<string> Prefixes { get; set; }

        public bool? Volatile { get; set; }

        public long? IdleTimeout { get; set; }

        public bool? Fifo { get; set; }

        public long? MaxMessageBytes { get; set; }

        public long? Ttl { get; set; }

        public long? MaxDeliveries { get; set; }

        internal string ToJson()
        {
            var fields = new List<string>
            {
                "\"name\":" + Json.Quote(Name ?? string.Empty),
                "\"type\":" + Json.Quote(Type ?? string.Empty),
            };
            if (Tags != null && Tags.Count > 0)
            {
                fields.Add("\"tags\":" + JsonWriter.Map(Tags));
            }
            if (Prefixes != null && Prefixes.Count > 0)
            {
                fields.Add("\"prefixes\":" + JsonWriter.List(Prefixes));
            }
            if (Volatile.HasValue)
            {
                fields.Add("\"volatile\":" + JsonWriter.Bool(Volatile.Value));
            }
            if (IdleTimeout.HasValue)
            {
                fields.Add("\"idleTimeout\":" + JsonWriter.Number(IdleTimeout.Value));
            }
            if (Fifo.HasValue)
            {
                fields.Add("\"fifo\":" + JsonWriter.Bool(Fifo.Value));
            }
            if (MaxMessageBytes.HasValue)
            {
                fields.Add("\"maxMessageBytes\":" + JsonWriter.Number(MaxMessageBytes.Value));
            }
            if (Ttl.HasValue)
            {
                fields.Add("\"ttl\":" + JsonWriter.Number(Ttl.Value));
            }
            if (MaxDeliveries.HasValue)
            {
                fields.Add("\"maxDeliveries\":" + JsonWriter.Number(MaxDeliveries.Value));
            }
            return "{" + string.Join(",", fields.ToArray()) + "}";
        }
    }

    /// <summary><c>PATCH /topics/{topic}</c>: só o que foi preenchido é enviado.</summary>
    public sealed class TopicUpdate
    {
        /// <summary>Substitui o mapa inteiro (um mapa vazio limpa as tags).</summary>
        public IDictionary<string, string> Tags { get; set; }

        public IList<string> Prefixes { get; set; }

        public long? IdleTimeout { get; set; }

        public Field<long> MaxMessageBytes { get; set; }

        public Field<long> Ttl { get; set; }

        public Field<long> MaxDeliveries { get; set; }

        internal string ToJson()
        {
            var fields = new List<string>();
            if (Tags != null)
            {
                fields.Add("\"tags\":" + JsonWriter.Map(Tags));
            }
            if (Prefixes != null)
            {
                fields.Add("\"prefixes\":" + JsonWriter.List(Prefixes));
            }
            if (IdleTimeout.HasValue)
            {
                fields.Add("\"idleTimeout\":" + JsonWriter.Number(IdleTimeout.Value));
            }
            MaxMessageBytes.Push("maxMessageBytes", fields);
            Ttl.Push("ttl", fields);
            MaxDeliveries.Push("maxDeliveries", fields);
            return "{" + string.Join(",", fields.ToArray()) + "}";
        }
    }

    /// <summary>As opções de <c>publish</c>.</summary>
    public sealed class PublishOptions
    {
        /// <summary>Padrão <c>application/json</c>.</summary>
        public string ContentType { get; set; }

        public string GroupId { get; set; }

        /// <summary>Só na fila FIFO.</summary>
        public string DeduplicationId { get; set; }

        public string RequestId { get; set; }

        /// <summary>Cabeçalho <c>traceparent</c> (W3C Trace Context), se a aplicação usa.</summary>
        public string Traceparent { get; set; }

        /// <summary>Marque quando o tópico é uma fila FIFO: só nela o <c>502</c>/<c>504</c> pode ser repetido.</summary>
        public bool Fifo { get; set; }
    }

    /// <summary>
    /// As opções de <c>consume</c>. O conector manda só o que foi informado: o que faltar vale o padrão
    /// do servidor.
    /// </summary>
    public sealed class ConsumeOptions
    {
        public int? MaxMessages { get; set; }

        /// <summary>Segundos de invisibilidade das mensagens entregues.</summary>
        public int? VisibilityTimeout { get; set; }

        /// <summary>Segundos de long polling.</summary>
        public int? WaitTime { get; set; }

        public bool AutoCreate { get; set; }

        /// <summary>Os quatro seguintes só valem com <see cref="AutoCreate"/>.</summary>
        public bool? Volatile { get; set; }

        public int? IdleTimeout { get; set; }

        public bool? Fifo { get; set; }

        public IDictionary<string, string> Tags { get; set; }

        public string RequestId { get; set; }

        public string Traceparent { get; set; }

        internal ConsumeOptions Clone()
        {
            return (ConsumeOptions)MemberwiseClone();
        }
    }
}
