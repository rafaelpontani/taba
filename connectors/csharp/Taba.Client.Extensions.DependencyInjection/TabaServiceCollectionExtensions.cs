using System;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;
using Taba;

namespace Microsoft.Extensions.DependencyInjection
{
    /// <summary>
    /// <c>AddTabaClient</c>: registra o <see cref="ITabaClient"/> como <em>singleton</em> (um só
    /// <see cref="TabaClient"/> por aplicação, descartado pelo contêiner), com o <see cref="TabaConfig"/>
    /// vindo do <c>IOptions&lt;TabaConfig&gt;</c>. Se houver um <see cref="ITabaJsonSerializer"/> registrado e
    /// o <see cref="TabaConfig.JsonSerializer"/> não foi informado, ele é usado no <c>PublishJson</c> e no
    /// <c>Consume&lt;T&gt;</c>.
    /// </summary>
    public static class TabaServiceCollectionExtensions
    {
        /// <summary>Registra o cliente; a configuração vem de <c>services.Configure&lt;TabaConfig&gt;(...)</c>.</summary>
        public static IServiceCollection AddTabaClient(this IServiceCollection services)
        {
            if (services == null)
            {
                throw new ArgumentNullException("services");
            }
            services.AddOptions<TabaConfig>();
            services.TryAddSingleton<ITabaClient>(provider =>
            {
                TabaConfig config = provider.GetRequiredService<IOptions<TabaConfig>>().Value;
                if (config.JsonSerializer == null)
                {
                    config.JsonSerializer = provider.GetService<ITabaJsonSerializer>();
                }
                return new TabaClient(config);
            });
            return services;
        }

        /// <summary>Registra o cliente e preenche o <see cref="TabaConfig"/> no código.</summary>
        public static IServiceCollection AddTabaClient(this IServiceCollection services, Action<TabaConfig> configure)
        {
            if (configure == null)
            {
                throw new ArgumentNullException("configure");
            }
            AddTabaClient(services);
            services.Configure(configure);
            return services;
        }

        /// <summary>
        /// Registra o cliente e preenche o <see cref="TabaConfig"/> da seção <paramref name="sectionPath"/>
        /// (por exemplo <c>"Taba"</c> ou <c>"Servicos:Taba"</c>) do <see cref="IConfiguration"/> do contêiner. O
        /// <see cref="IConfiguration"/> só é pedido ao contêiner quando o <see cref="ITabaClient"/> é resolvido
        /// pela primeira vez, e não na hora do registro: não exige o <see cref="IConfiguration"/> em mãos
        /// durante a montagem dos serviços e evita dependência circular com quem o monta.
        /// </summary>
        public static IServiceCollection AddTabaClient(this IServiceCollection services, string sectionPath)
        {
            if (string.IsNullOrWhiteSpace(sectionPath))
            {
                throw new ArgumentException("informe o caminho da seção (ex.: \"Taba\")", "sectionPath");
            }
            AddTabaClient(services);
            services.AddOptions<TabaConfig>()
                .Configure<IConfiguration>((config, configuration) => configuration.GetSection(sectionPath).Bind(config));
            return services;
        }

        /// <summary>
        /// Registra o cliente e preenche o <see cref="TabaConfig"/> de uma seção da configuração
        /// (<c>appsettings.json</c>: <c>BaseUrl</c>, <c>Token</c>, <c>RequestTimeout</c> como <c>"00:00:30"</c>...).
        /// </summary>
        public static IServiceCollection AddTabaClient(this IServiceCollection services, IConfiguration section)
        {
            if (section == null)
            {
                throw new ArgumentNullException("section");
            }
            AddTabaClient(services);
            services.Configure<TabaConfig>(section);
            return services;
        }
    }
}
