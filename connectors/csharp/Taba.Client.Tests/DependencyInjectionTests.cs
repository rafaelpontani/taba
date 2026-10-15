using System;
using System.IO;
using System.Text;
using Microsoft.Extensions.Configuration;
using Microsoft.Extensions.DependencyInjection;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary><c>AddTabaClient</c>: the <see cref="ITabaClient"/> as a singleton, with the <see cref="TabaConfig"/> from the options.</summary>
    [TestFixture]
    public class DependencyInjectionTests
    {
        private sealed class FixedSerializer : ITabaJsonSerializer
        {
            public string Serialize(object value)
            {
                return "{}";
            }

            public T Deserialize<T>(string json)
            {
                return default(T);
            }
        }

        /// UC: UC-CNT-035
        [Test]
        public void add_taba_client_registers_one_singleton_with_the_config_from_code()
        {
            var services = new ServiceCollection();
            services.AddTabaClient(config =>
            {
                config.BaseUrl = "http://taba.test:8080/";
                config.Token = "tb_x";
                config.MaxAttempts = 5;
            });
            using (ServiceProvider provider = services.BuildServiceProvider())
            {
                ITabaClient first = provider.GetRequiredService<ITabaClient>();
                Assert.That(provider.GetRequiredService<ITabaClient>(), Is.SameAs(first), "only one per application");
                var client = (TabaClient)first;
                Assert.That(client.Config.BaseUrl, Is.EqualTo("http://taba.test:8080/"));
                Assert.That(client.Config.Token, Is.EqualTo("tb_x"));
                Assert.That(client.Config.MaxAttempts, Is.EqualTo(5));
            }
        }

        /// UC: UC-CNT-035
        [Test]
        public void add_taba_client_binds_a_configuration_section_and_uses_a_registered_serializer()
        {
            string json = "{\"Taba\":{\"BaseUrl\":\"https://taba.internal:8443\",\"Token\":\"tb_y\",\"RequestTimeout\":\"00:00:45\",\"MaxAttempts\":2}}";
            IConfiguration configuration = new ConfigurationBuilder()
                .AddJsonStream(new MemoryStream(Encoding.UTF8.GetBytes(json)))
                .Build();
            var services = new ServiceCollection();
            services.AddSingleton<ITabaJsonSerializer, FixedSerializer>();
            services.AddTabaClient(configuration.GetSection("Taba"));
            using (ServiceProvider provider = services.BuildServiceProvider())
            {
                var client = (TabaClient)provider.GetRequiredService<ITabaClient>();
                Assert.That(client.Config.BaseUrl, Is.EqualTo("https://taba.internal:8443"));
                Assert.That(client.Config.Token, Is.EqualTo("tb_y"));
                Assert.That(client.Config.RequestTimeout, Is.EqualTo(TimeSpan.FromSeconds(45)));
                Assert.That(client.Config.MaxAttempts, Is.EqualTo(2));
                Assert.That(client.Config.JsonSerializer, Is.InstanceOf<FixedSerializer>(), "the container's serializer applies");
            }
        }

        /// UC: UC-CNT-035
        [Test]
        public void add_taba_client_with_a_section_path_reads_the_configuration_only_when_the_client_is_resolved()
        {
            var services = new ServiceCollection();
            services.AddTabaClient("Services:Taba");   // the IConfiguration has not even been registered yet

            string json = "{\"Services\":{\"Taba\":{\"BaseUrl\":\"http://taba.test:8080\",\"Token\":\"tb_z\"}}}";
            IConfiguration configuration = new ConfigurationBuilder()
                .AddJsonStream(new MemoryStream(Encoding.UTF8.GetBytes(json)))
                .Build();
            services.AddSingleton(configuration);      // registered later, before resolving

            using (ServiceProvider provider = services.BuildServiceProvider())
            {
                var client = (TabaClient)provider.GetRequiredService<ITabaClient>();
                Assert.That(client.Config.BaseUrl, Is.EqualTo("http://taba.test:8080"));
                Assert.That(client.Config.Token, Is.EqualTo("tb_z"));
            }
            Assert.Throws<ArgumentException>(() => new ServiceCollection().AddTabaClient(" "));
        }

        /// UC: UC-CNT-035
        [Test]
        public void an_invalid_base_url_fails_when_the_client_is_resolved_with_client_invalid_config()
        {
            var services = new ServiceCollection();
            services.AddTabaClient(config => config.BaseUrl = "this is not an address");
            using (ServiceProvider provider = services.BuildServiceProvider())
            {
                var e = Assert.Throws<TabaException>(() => provider.GetRequiredService<ITabaClient>());
                Assert.That(e.Code, Is.EqualTo("client.invalid-config"));
            }
        }
    }
}
