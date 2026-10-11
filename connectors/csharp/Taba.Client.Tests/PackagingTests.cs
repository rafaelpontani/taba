using System;
using System.IO;
using System.Linq;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary>
    /// O projeto do conector e o código-fonte dizem o que a documentação promete (doc 19, seções 18.5 e
    /// 18.6): os alvos, o C# 7.3, nenhuma dependência, a TLS da plataforma e o <c>PATCH</c> de verdade.
    /// O build de cada alvo é o que confere a compilação de fato.
    /// </summary>
    [TestFixture]
    public class PackagingTests
    {
        private static string Library()
        {
            for (var dir = new DirectoryInfo(AppContext.BaseDirectory); dir != null; dir = dir.Parent)
            {
                string candidate = Path.Combine(dir.FullName, "Taba.Client");
                if (File.Exists(Path.Combine(candidate, "Taba.Client.csproj")))
                {
                    return candidate;
                }
            }
            throw new InvalidOperationException("pasta Taba.Client não encontrada");
        }

        private static string[] Sources()
        {
            return new[] { Library(), Path.Combine(Path.GetDirectoryName(Library()), "Taba.Client.Abstractions") }
                .SelectMany(d => Directory.GetFiles(d, "*.cs", SearchOption.AllDirectories))
                .Where(f => !f.Contains(Path.DirectorySeparatorChar + "obj" + Path.DirectorySeparatorChar)
                    && !f.Contains(Path.DirectorySeparatorChar + "bin" + Path.DirectorySeparatorChar))
                .ToArray();
        }

        /// UC: UC-CNT-024
        [Test]
        public void the_library_targets_classic_and_modern_dotnet_in_csharp_7_3()
        {
            string project = File.ReadAllText(Path.Combine(Library(), "Taba.Client.csproj"));
            foreach (string target in new[] { "net461", "net5.0", "net6.0", "net8.0" })
            {
                Assert.That(project, Does.Contain(target), target);
            }
            Assert.That(project, Does.Contain("net10.0"), "o net10.0 entra com o SDK 10");
            Assert.That(project, Does.Contain("<LangVersion>7.3</LangVersion>"));
            Assert.That(project, Does.Contain("<TreatWarningsAsErrors>true</TreatWarningsAsErrors>"));
        }

        /// UC: UC-CNT-023
        [Test]
        public void the_only_package_is_the_reference_assemblies_for_the_classic_target()
        {
            string project = File.ReadAllText(Path.Combine(Library(), "Taba.Client.csproj"));
            var packages = project.Split(new[] { "<PackageReference " }, StringSplitOptions.None).Skip(1).ToArray();
            Assert.That(packages.Length, Is.EqualTo(1));
            Assert.That(packages[0], Does.StartWith("Include=\"Microsoft.NETFramework.ReferenceAssemblies\""));
            Assert.That(packages[0].Split('>')[0], Does.Contain("PrivateAssets=\"all\""), "não vira dependência de quem usa");
            Assert.That(project, Does.Contain("<ProjectReference Include=\"..\\Taba.Client.Abstractions\\Taba.Client.Abstractions.csproj\" />"), "a única dependência é o pacote dos contratos, do próprio conector");
            Assert.That(project.Split(new[] { "<ProjectReference " }, StringSplitOptions.None).Length, Is.EqualTo(2));
        }

        /// UC: UC-CNT-030
        [Test]
        public void tls_is_the_platforms_and_certificate_checks_are_never_turned_off()
        {
            foreach (string file in Sources())
            {
                string code = File.ReadAllText(file);
                foreach (string forbidden in new[]
                {
                    "ServerCertificateCustomValidationCallback", "ServerCertificateValidationCallback",
                    "DangerousAcceptAnyServerCertificateValidator", "SecurityProtocol =", "SslProtocols",
                    "CheckCertificateRevocationList = false",
                })
                {
                    Assert.That(code, Does.Not.Contain(forbidden), Path.GetFileName(file));
                }
            }
        }

        /// UC: UC-CNT-029
        [Test]
        public void patch_uses_the_http_method_and_never_the_override_header()
        {
            string client = File.ReadAllText(Path.Combine(Library(), "TabaClient.cs"));
            Assert.That(client, Does.Contain("new HttpMethod(\"PATCH\")"));
            Assert.That(client, Does.Not.Contain("X-HTTP-Method-Override"));
            Assert.That(client, Does.Not.Contain("HttpWebRequest"), "sem conexão HTTP própria para esse verbo");
        }
    }
}
