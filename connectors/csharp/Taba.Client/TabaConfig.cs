using System;

namespace Taba
{
    /// <summary>
    /// The client configuration (doc 19, section 18.3). It does not read environment variables: <c>TABA_*</c> is the
    /// prefix of the server configuration; whoever uses the connector reads their own and fills this object.
    /// </summary>
    public sealed class TabaConfig
    {
        public TabaConfig()
        {
        }

        public TabaConfig(string baseUrl)
        {
            BaseUrl = baseUrl;
        }

        /// <summary>Address of the client API load balancer (<c>http://host:port</c> or <c>https://...</c>). Required.</summary>
        public string BaseUrl { get; set; }

        /// <summary>Goes in <c>Authorization: Bearer</c>. Without it, the call goes without the header.</summary>
        public string Token { get; set; }

        /// <summary>
        /// Time to open the connection (5 s). Applies on .NET 5 and later; on .NET Framework the
        /// <c>HttpClient</c> has no such separate limit, and only <see cref="RequestTimeout"/> applies.
        /// </summary>
        public TimeSpan ConnectTimeout { get; set; } = TimeSpan.FromSeconds(5);

        /// <summary>Total time of an ordinary call (30 s). <c>consume</c> uses <c>waitTime + 10 s</c>.</summary>
        public TimeSpan RequestTimeout { get; set; } = TimeSpan.FromSeconds(30);

        /// <summary>Tentativas, contando a primeira (3).</summary>
        public int MaxAttempts { get; set; } = 3;

        /// <summary>Goes in <c>User-Agent</c> (<c>taba-csharp/&lt;version&gt;</c>). Add the application name.</summary>
        public string UserAgent { get; set; } = "taba-csharp/" + TabaClient.Version;

        /// <summary>Who serializes the <c>PublishJson</c> object. Without it, the connector default applies (<c>System.Text.Json</c> on .NET 5 and later, <c>DataContractJsonSerializer</c> on .NET Framework).</summary>
        public ITabaJsonSerializer JsonSerializer { get; set; }

        internal TabaConfig Copy()
        {
            return (TabaConfig)MemberwiseClone();
        }
    }
}
