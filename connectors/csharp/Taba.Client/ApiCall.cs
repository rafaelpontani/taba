using System;
using System.Collections.Generic;
using System.Globalization;
using System.Net.Http;

namespace Taba
{
    /// <summary>
    /// A descrição de uma requisição à API: método, caminho, query, corpo, cabeçalhos e as regras da
    /// chamada (tempo limite, repetição de 502/504, respostas que valem sucesso depois de um 503). O
    /// <see cref="HttpTransport"/> monta um <c>HttpRequestMessage</c> novo dela a cada tentativa, porque
    /// um <c>HttpRequestMessage</c> não pode ser enviado duas vezes.
    /// </summary>
    internal sealed class ApiCall
    {
        public ApiCall(HttpMethod method, string path)
        {
            Method = method;
            Path = path;
        }

        public HttpMethod Method { get; private set; }

        /// <summary>Caminho já com os segmentos codificados (<c>/topics/pedidos</c>).</summary>
        public string Path { get; private set; }

        public List<KeyValuePair<string, string>> Query { get; } = new List<KeyValuePair<string, string>>();

        public byte[] Body { get; set; }

        public string ContentType { get; set; }

        public string RequestId { get; set; }

        public string Traceparent { get; set; }

        /// <summary>Troca o tempo total da chamada (o <c>consume</c> usa <c>waitTime + 10 s</c>).</summary>
        public TimeSpan? Timeout { get; set; }

        /// <summary>Repete <c>502</c> e <c>504</c>: só a publicação em fila FIFO, em que o primário deduplica.</summary>
        public bool RetryGateway { get; set; }

        /// <summary>Depois de um <c>503</c>, estas respostas contam como sucesso (a operação já tinha sido aplicada).</summary>
        public List<KeyValuePair<int, string>> Recover { get; } = new List<KeyValuePair<int, string>>();

        public ApiCall With(string key, object value)
        {
            Query.Add(new KeyValuePair<string, string>(key, Format(value)));
            return this;
        }

        public ApiCall Json(string json)
        {
            Body = Utf8.Encoding.GetBytes(json);
            return this;
        }
    internal static string Format(object value)
    {
        if (value is bool)
        {
            return (bool)value ? "true" : "false";
        }
        return Convert.ToString(value, CultureInfo.InvariantCulture);
    }

    internal static string Segment(string value)
    {
        return Uri.EscapeDataString(value);
    }
    }

    /// <summary>A resposta já lida de uma <see cref="ApiCall"/>.</summary>
    internal sealed class ApiReply
    {
        public int Status { get; set; }

        public byte[] Body { get; set; }

        public bool Recovered { get; set; }

        public string Text
        {
            get { return Utf8.Encoding.GetString(Body); }
        }
    }
}
