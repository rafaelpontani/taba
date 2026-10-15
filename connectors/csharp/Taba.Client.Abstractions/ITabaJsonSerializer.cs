namespace Taba
{
    /// <summary>
    /// What converts between the application object and the JSON text: serializes in <c>PublishJson</c> and
    /// deserializes in <c>Consume&lt;T&gt;</c>. The connector ships a default (<c>System.Text.Json</c> on .NET 5
    /// and later, <c>DataContractJsonSerializer</c> on .NET Framework); to use another (Newtonsoft.Json,
    /// for example), set it in <c>TabaConfig.JsonSerializer</c>.
    /// </summary>
    public interface ITabaJsonSerializer
    {
        string Serialize(object value);

        T Deserialize<T>(string json);
    }
}
