using System;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection.Extensions;
using Microsoft.Extensions.Options;
using Taba;

namespace Microsoft.Extensions.DependencyInjection
{
    /// <summary>
    /// <c>AddTabaClient</c>: registers <see cref="ITabaClient"/> as a <em>singleton</em> (a single
    /// <see cref="TabaClient"/> per application, disposed by the container), with the <see cref="TabaConfig"/>
    /// coming from <c>IOptions&lt;TabaConfig&gt;</c>. If an <see cref="ITabaJsonSerializer"/> is registered and
    /// <see cref="TabaConfig.JsonSerializer"/> was not set, it is used in <c>PublishJson</c> and in
    /// <c>Consume&lt;T&gt;</c>.
    /// </summary>
    public static class TabaServiceCollectionExtensions
    {
        /// <summary>Registers the client; the configuration comes from <c>services.Configure&lt;TabaConfig&gt;(...)</c>.</summary>
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

        /// <summary>Registers the client and fills the <see cref="TabaConfig"/> in code.</summary>
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
        /// Registers the client and fills the <see cref="TabaConfig"/> from the <paramref name="sectionPath"/> section
        /// (for example <c>"Taba"</c> or <c>"Services:Taba"</c>) of the container's <see cref="IConfiguration"/>. The
        /// <see cref="IConfiguration"/> is only requested from the container when the <see cref="ITabaClient"/> is resolved
        /// for the first time, not at registration time: it does not require the <see cref="IConfiguration"/> at hand
        /// while the services are being assembled and avoids a circular dependency with whoever assembles it.
        /// </summary>
        public static IServiceCollection AddTabaClient(this IServiceCollection services, string sectionPath)
        {
            if (string.IsNullOrWhiteSpace(sectionPath))
            {
                throw new ArgumentException("provide the section path (e.g. \"Taba\")", "sectionPath");
            }
            AddTabaClient(services);
            services.AddOptions<TabaConfig>()
                .Configure<IConfiguration>((config, configuration) => configuration.GetSection(sectionPath).Bind(config));
            return services;
        }

        /// <summary>
        /// Registers the client and fills the <see cref="TabaConfig"/> from a configuration section
        /// (<c>appsettings.json</c>: <c>BaseUrl</c>, <c>Token</c>, <c>RequestTimeout</c> like <c>"00:00:30"</c>...).
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
