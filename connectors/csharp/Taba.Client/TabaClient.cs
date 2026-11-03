using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// The Taba client API client (doc 19, section 18.3): the nine operations, all with the
    /// asynchronous form (<c>...Async</c>, which is the implementation) and the synchronous one (which waits for the asynchronous), and
    /// <c>subscribe</c>. It is safe for concurrent use: a single object serves several tasks and threads.
    /// </summary>
    /// <remarks>
    /// TLS: the connector uses the platform's and does not change it. On .NET Framework 4.6.x the protocol is the one in
    /// <c>ServicePointManager.SecurityProtocol</c> (check that it includes TLS 1.2); from 4.7 on it is the
    /// system's. Certificate verification is never turned off.
    /// </remarks>
    public sealed partial class TabaClient : ITabaClient, ITabaBatchClient
    {
        /// <summary>The version of this connector.</summary>
        public const string Version = "0.1.0";

        /// <summary>The version of the API contract this connector implements.</summary>
        public const string ContractVersion = "1";

        /// <summary><c>consume</c> waits <c>waitTime</c> plus this slack, not the <c>RequestTimeout</c>.</summary>
        internal static readonly TimeSpan LongPollSlack = TimeSpan.FromSeconds(10);

        private static readonly HttpMethod Patch = new HttpMethod("PATCH");

        internal const int DefaultHandlerMaxMessages = 1;
        internal const int DefaultHandlerVisibilityTimeout = 120;
        internal const int DefaultHandlerWaitTime = 20;

        private static readonly ITabaJsonSerializer DefaultSerializer = new DefaultJsonSerializer();

        private readonly TabaConfig _config;
        private readonly HttpTransport _transport;

        public TabaClient(TabaConfig config)
            : this(config, null, null)
        {
        }

        /// <summary>
        /// To swap the network stack: an own <see cref="HttpMessageHandler"/> (a proxy, a
        /// fake transport in the tests of whoever uses the connector). The client does not dispose it.
        /// </summary>
        public TabaClient(TabaConfig config, HttpMessageHandler handler)
            : this(config, handler, null)
        {
        }

        internal TabaClient(TabaConfig config, HttpMessageHandler handler, Func<TimeSpan, CancellationToken, Task> delay)
        {
            if (config == null)
            {
                throw new ArgumentNullException("config");
            }
            _config = config.Copy();
            _transport = new HttpTransport(_config, handler, delay);
        }

        /// <summary>A copy of the configuration in use.</summary>
        public TabaConfig Config
        {
            get { return _config.Copy(); }
        }

        private static string Segment(string value)
        {
            return ApiCall.Segment(value);
        }

        private Task<ApiReply> SendAsync(ApiCall call, CancellationToken cancellationToken)
        {
            return _transport.SendAsync(call, cancellationToken);
        }

        public void Dispose()
        {
            _transport.Dispose();
        }
    }
}
