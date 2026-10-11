using System;
using System.IO;
#if !NET5_0_OR_GREATER
using System.Runtime.Serialization.Json;
#endif
using System.Text;

namespace Taba
{
    /// <summary>
    /// O serializador padrão do <c>PublishJson</c>, sem dependência extra: <c>System.Text.Json</c> (nomes em
    /// camelCase) no .NET 5 em diante e <c>DataContractJsonSerializer</c> no .NET Framework (nomes como
    /// declarados; datas e tipos especiais seguem as regras dele). Quem precisa de mais informa o seu em
    /// <see cref="TabaConfig.JsonSerializer"/>.
    /// </summary>
    internal sealed class DefaultJsonSerializer : ITabaJsonSerializer
    {
#if NET5_0_OR_GREATER
        private static readonly System.Text.Json.JsonSerializerOptions Options =
            new System.Text.Json.JsonSerializerOptions(System.Text.Json.JsonSerializerDefaults.Web);

        public string Serialize(object value)
        {
            return System.Text.Json.JsonSerializer.Serialize(value, value.GetType(), Options);
        }

        public T Deserialize<T>(string json)
        {
            return System.Text.Json.JsonSerializer.Deserialize<T>(json, Options);
        }
#else
        public string Serialize(object value)
        {
            var serializer = new DataContractJsonSerializer(
                value.GetType(),
                new DataContractJsonSerializerSettings { UseSimpleDictionaryFormat = true });
            using (var stream = new MemoryStream())
            {
                serializer.WriteObject(stream, value);
                return Utf8.Encoding.GetString(stream.ToArray());
            }
        }

        public T Deserialize<T>(string json)
        {
            var serializer = new DataContractJsonSerializer(
                typeof(T),
                new DataContractJsonSerializerSettings { UseSimpleDictionaryFormat = true });
            using (var stream = new MemoryStream(Utf8.Encoding.GetBytes(json)))
            {
                return (T)serializer.ReadObject(stream);
            }
        }
#endif
    }
}
