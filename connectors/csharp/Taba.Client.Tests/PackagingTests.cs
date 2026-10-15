using System;
using System.IO;
using System.Linq;
using NUnit.Framework;

namespace Taba.Tests
{
    /// <summary>
    /// The connector project and the source code say what the documentation promises (doc 19, sections 18.5 and
    /// 18.6): the targets, C# 7.3, no dependencies, the platform's TLS and a real <c>PATCH</c>.
    /// The build of each target is what actually checks the compilation.
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
            throw new InvalidOperationException("Taba.Client folder not found");
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
            Assert.That(project, Does.Contain("net10.0"), "net10.0 comes in with SDK 10");
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
            Assert.That(packages[0].Split('>')[0], Does.Contain("PrivateAssets=\"all\""), "it does not become a dependency of whoever uses it");
            Assert.That(project, Does.Contain("<ProjectReference Include=\"..\\Taba.Client.Abstractions\\Taba.Client.Abstractions.csproj\" />"), "the only dependency is the contracts package, from the connector itself");
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
            Assert.That(client, Does.Not.Contain("HttpWebRequest"), "no HTTP connection of its own for that verb");
        }
    }
}
