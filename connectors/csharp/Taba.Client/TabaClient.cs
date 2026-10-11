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
    /// O cliente da API de cliente do Taba (doc 19, seção 18.3): as nove operações, todas com a forma
    /// assíncrona (<c>...Async</c>, que é a implementação) e a síncrona (que espera pela assíncrona), e o
    /// <c>subscribe</c>. É seguro para uso concorrente: um só objeto serve a várias tarefas e threads.
    /// </summary>
    /// <remarks>
    /// TLS: o conector usa o da plataforma e não o altera. No .NET Framework 4.6.x o protocolo é o do
    /// <c>ServicePointManager.SecurityProtocol</c> (confira que inclui o TLS 1.2); do 4.7 em diante é o
    /// do sistema. A verificação do certificado nunca é desligada.
    /// </remarks>
    public sealed partial class TabaClient : ITabaClient
    {
        /// <summary>A versão deste conector.</summary>
        public const string Version = "0.1.0";

        /// <summary>A versão do contrato da API que este conector implementa.</summary>
        public const string ContractVersion = "1";

        /// <summary>O <c>consume</c> espera <c>waitTime</c> mais esta folga, e não o <c>RequestTimeout</c>.</summary>
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
        /// Para trocar a pilha de rede: um <see cref="HttpMessageHandler"/> próprio (um proxy, um
        /// transporte de mentira nos testes de quem usa o conector). O cliente não o descarta.
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

        /// <summary>Uma cópia da configuração em uso.</summary>
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
