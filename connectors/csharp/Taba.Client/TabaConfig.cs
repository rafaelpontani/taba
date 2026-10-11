using System;

namespace Taba
{
    /// <summary>
    /// A configuração do cliente (doc 19, seção 18.3). Não lê variáveis de ambiente: <c>TABA_*</c> é o
    /// prefixo da configuração do servidor; quem usa o conector lê as suas e preenche este objeto.
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

        /// <summary>Endereço do balanceador da API de cliente (<c>http://host:porta</c> ou <c>https://...</c>). Obrigatório.</summary>
        public string BaseUrl { get; set; }

        /// <summary>Vai em <c>Authorization: Bearer</c>. Sem ele, a chamada vai sem o cabeçalho.</summary>
        public string Token { get; set; }

        /// <summary>
        /// Tempo para abrir a conexão (5 s). Vale no .NET 5 em diante; no .NET Framework o
        /// <c>HttpClient</c> não tem esse limite separado, e vale só o <see cref="RequestTimeout"/>.
        /// </summary>
        public TimeSpan ConnectTimeout { get; set; } = TimeSpan.FromSeconds(5);

        /// <summary>Tempo total de uma chamada comum (30 s). O <c>consume</c> usa <c>waitTime + 10 s</c>.</summary>
        public TimeSpan RequestTimeout { get; set; } = TimeSpan.FromSeconds(30);

        /// <summary>Tentativas, contando a primeira (3).</summary>
        public int MaxAttempts { get; set; } = 3;

        /// <summary>Vai em <c>User-Agent</c> (<c>taba-csharp/&lt;versão&gt;</c>). Acrescente o nome do aplicativo.</summary>
        public string UserAgent { get; set; } = "taba-csharp/" + TabaClient.Version;

        internal TabaConfig Copy()
        {
            return (TabaConfig)MemberwiseClone();
        }
    }
}
