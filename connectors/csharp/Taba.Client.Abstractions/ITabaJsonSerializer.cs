namespace Taba
{
    /// <summary>
    /// Quem converte entre o objeto da aplicação e o texto JSON: serializa no <c>PublishJson</c> e
    /// deserializa no <c>Consume&lt;T&gt;</c>. O conector traz um padrão (<c>System.Text.Json</c> no .NET 5
    /// em diante, <c>DataContractJsonSerializer</c> no .NET Framework); para usar outro (Newtonsoft.Json,
    /// por exemplo), informe-o em <c>TabaConfig.JsonSerializer</c>.
    /// </summary>
    public interface ITabaJsonSerializer
    {
        string Serialize(object value);

        T Deserialize<T>(string json);
    }
}
