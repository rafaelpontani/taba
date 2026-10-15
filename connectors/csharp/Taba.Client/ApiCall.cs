using System;
using System.Collections.Generic;
using System.Globalization;
using System.Net.Http;

namespace Taba
{
    /// <summary>
    /// The description of a request to the API: method, path, query, body, headers and the rules of the
    /// call (timeout, retry of 502/504, responses that count as success after a 503). The
    /// <see cref="HttpTransport"/> builds a new <c>HttpRequestMessage</c> from it on every attempt, because
    /// an <c>HttpRequestMessage</c> cannot be sent twice.
    /// </summary>
    internal sealed class ApiCall
    {
        public ApiCall(HttpMethod method, string path)
        {
            Method = method;
            Path = path;
        }

        public HttpMethod Method { get; private set; }

        /// <summary>Path with the segments already encoded (<c>/topics/orders</c>).</summary>
        public string Path { get; private set; }

        public List<KeyValuePair<string, string>> Query { get; } = new List<KeyValuePair<string, string>>();

        public byte[] Body { get; set; }

        public string ContentType { get; set; }

        public string RequestId { get; set; }

        public string Traceparent { get; set; }

        /// <summary>Overrides the total time of the call (<c>consume</c> uses <c>waitTime + 10 s</c>).</summary>
        public TimeSpan? Timeout { get; set; }

        /// <summary>Retries <c>502</c> and <c>504</c>: only publishing to a FIFO queue, where the primary deduplicates.</summary>
        public bool RetryGateway { get; set; }

        /// <summary>After a <c>503</c>, these responses count as success (the operation had already been applied).</summary>
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

    /// <summary>The already-read response of an <see cref="ApiCall"/>.</summary>
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
