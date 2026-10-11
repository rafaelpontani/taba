using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// O contrato do cliente da API de cliente do Taba (doc 19, seção 18.3): as nove operações, nas
    /// formas assíncrona e síncrona, e o <c>subscribe</c>. Dependa desta interface (e não do
    /// <c>TabaClient</c>) para injetar o cliente e para simulá-lo nos testes.
    /// </summary>
    public interface ITabaClient : IDisposable
    {
        /// <summary><c>POST /topics</c>.</summary>
        Task<Topic> CreateTopicAsync(TopicRequest request, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics/{topic}</c>.</summary>
        Task<Topic> GetTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics</c>. Os parâmetros <c>null</c> não vão na query.</summary>
        Task<Page<Topic>> ListTopicsAsync(int? limit = null, string cursor = null, string name = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Todos os tópicos, página a página.</summary>
        Task<IList<Topic>> ListAllTopicsAsync(string name = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>PATCH /topics/{topic}</c>.</summary>
        Task<Topic> UpdateTopicAsync(string name, TopicUpdate update, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>DELETE /topics/{topic}</c>.</summary>
        Task DeleteTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>POST /topics/{topic}/messages</c>. O corpo vai como bytes, sem validação.</summary>
        Task<PublishResponse> PublishAsync(string topic, byte[] body, PublishOptions options = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Serializa o objeto em JSON e publica (<c>Content-Type: application/json</c>). Para serializar por conta própria, use o <see cref="PublishAsync"/>.</summary>
        Task<PublishResponse> PublishJsonAsync<T>(string topic, T value, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Publica um texto JSON que a aplicação já montou, sem serializar.</summary>
        Task<PublishResponse> PublishRawJsonAsync(string topic, string json, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Publica um texto (<c>Content-Type: text/plain; charset=utf-8</c>).</summary>
        Task<PublishResponse> PublishTextAsync(string topic, string text, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics/{topic}/messages</c>. Devolve uma lista vazia se não há mensagens.</summary>
        Task<IList<Message>> ConsumeAsync(string topic, ConsumeOptions options = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>
        /// Lê da fila e entrega cada mensagem ao handler, com <see cref="MessageContext.Delete"/> (padrão sim)
        /// e <see cref="MessageContext.ChangeVisibilityAsync"/>; enquanto o handler roda, a invisibilidade é
        /// renovada sozinha. Devolve quantas mensagens foram tratadas.
        /// </summary>
        Task<int> ConsumeAsync(string topic, ConsumeOptions options, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>O consumo com handler, com o corpo já deserializado como <typeparamref name="T"/> em <see cref="MessageContext{T}.Body"/>.</summary>
        Task<int> ConsumeAsync<T>(string topic, ConsumeOptions options, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>O <c>ConsumeAsync&lt;T&gt;</c> sem opções.</summary>
        Task<int> ConsumeAsync<T>(string topic, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>O consumo com handler, sem opções: valem os padrões do servidor.</summary>
        Task<int> ConsumeAsync(string topic, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>DELETE /topics/{topic}/messages/{id}?receiptToken=</c>.</summary>
        Task AckAsync(string topic, string messageId, string receiptToken, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Confirma a mensagem recebida do <see cref="ConsumeAsync"/>.</summary>
        Task AckAsync(string topic, Message message, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>PUT /topics/{topic}/messages/{id}/visibility?receiptToken=</c>.</summary>
        Task<VisibilityResponse> ChangeVisibilityAsync(string topic, string messageId, string receiptToken, int seconds, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Muda a invisibilidade da mensagem recebida do <see cref="ConsumeAsync"/>.</summary>
        Task<VisibilityResponse> ChangeVisibilityAsync(string topic, Message message, int seconds, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>
        /// O laço de consumo: long polling, chama o <paramref name="handler"/> e confirma sozinho só se
        /// ele terminar sem exceção. Termina, sem exceção, quando o token é cancelado.
        /// </summary>
        Task SubscribeAsync(string topic, ConsumeOptions options, Func<Message, Task> handler, CancellationToken cancellationToken, Action<Message, Exception> onError = null);

        Topic CreateTopic(TopicRequest request);

        Topic GetTopic(string name);

        Page<Topic> ListTopics(int? limit = null, string cursor = null, string name = null);

        IList<Topic> ListAllTopics(string name = null);

        Topic UpdateTopic(string name, TopicUpdate update);

        void DeleteTopic(string name);

        PublishResponse Publish(string topic, byte[] body, PublishOptions options = null);

        PublishResponse PublishJson<T>(string topic, T value);

        PublishResponse PublishRawJson(string topic, string json);

        PublishResponse PublishText(string topic, string text);

        IList<Message> Consume(string topic, ConsumeOptions options = null);

        int Consume(string topic, ConsumeOptions options, Action<MessageContext> handler);

        int Consume(string topic, Action<MessageContext> handler);

        int Consume<T>(string topic, ConsumeOptions options, Action<MessageContext<T>> handler);

        int Consume<T>(string topic, Action<MessageContext<T>> handler);

        void Ack(string topic, string messageId, string receiptToken);

        void Ack(string topic, Message message);

        VisibilityResponse ChangeVisibility(string topic, string messageId, string receiptToken, int seconds);

        VisibilityResponse ChangeVisibility(string topic, Message message, int seconds);

        /// <summary>A forma síncrona do <see cref="SubscribeAsync"/>: bloqueia a thread até o cancelamento.</summary>
        void Subscribe(string topic, ConsumeOptions options, Action<Message> handler, CancellationToken cancellationToken, Action<Message, Exception> onError = null);
    }
}
