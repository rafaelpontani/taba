using System;
using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// The contract of the Taba client API client (doc 19, section 18.3): the nine operations, in the
    /// asynchronous and synchronous forms, and <c>subscribe</c>. Depend on this interface (and not on
    /// <c>TabaClient</c>) to inject the client and to mock it in tests.
    /// </summary>
    public interface ITabaClient : IDisposable
    {
        /// <summary><c>POST /topics</c>.</summary>
        Task<Topic> CreateTopicAsync(TopicRequest request, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics/{topic}</c>.</summary>
        Task<Topic> GetTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics</c>. <c>null</c> parameters do not go in the query.</summary>
        Task<Page<Topic>> ListTopicsAsync(int? limit = null, string cursor = null, string name = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>All topics, page by page.</summary>
        Task<IList<Topic>> ListAllTopicsAsync(string name = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>PATCH /topics/{topic}</c>.</summary>
        Task<Topic> UpdateTopicAsync(string name, TopicUpdate update, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>DELETE /topics/{topic}</c>.</summary>
        Task DeleteTopicAsync(string name, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>POST /topics/{topic}/messages</c>. The body goes as bytes, without validation.</summary>
        Task<PublishResponse> PublishAsync(string topic, byte[] body, PublishOptions options = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Serializes the object to JSON and publishes it (<c>Content-Type: application/json</c>). To serialize on your own, use <see cref="PublishAsync"/>.</summary>
        Task<PublishResponse> PublishJsonAsync<T>(string topic, T value, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Publishes a JSON text the application already built, without serializing.</summary>
        Task<PublishResponse> PublishRawJsonAsync(string topic, string json, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Publishes a text (<c>Content-Type: text/plain; charset=utf-8</c>).</summary>
        Task<PublishResponse> PublishTextAsync(string topic, string text, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>GET /topics/{topic}/messages</c>. Returns an empty list if there are no messages.</summary>
        Task<IList<Message>> ConsumeAsync(string topic, ConsumeOptions options = null, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>
        /// Reads from the queue and hands each message to the handler, with <see cref="MessageContext.Delete"/> (default yes)
        /// and <see cref="MessageContext.ChangeVisibilityAsync"/>; while the handler runs, the invisibility is
        /// renewed automatically. Returns how many messages were handled.
        /// </summary>
        Task<int> ConsumeAsync(string topic, ConsumeOptions options, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Consuming with a handler, with the body already deserialized as <typeparamref name="T"/> in <see cref="MessageContext{T}.Body"/>.</summary>
        Task<int> ConsumeAsync<T>(string topic, ConsumeOptions options, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>ConsumeAsync&lt;T&gt;</c> without options.</summary>
        Task<int> ConsumeAsync<T>(string topic, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Consuming with a handler, without options: the server defaults apply.</summary>
        Task<int> ConsumeAsync(string topic, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>DELETE /topics/{topic}/messages/{id}?receiptToken=</c>.</summary>
        Task AckAsync(string topic, string messageId, string receiptToken, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Acknowledges the message received from <see cref="ConsumeAsync"/>.</summary>
        Task AckAsync(string topic, Message message, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary><c>PUT /topics/{topic}/messages/{id}/visibility?receiptToken=</c>.</summary>
        Task<VisibilityResponse> ChangeVisibilityAsync(string topic, string messageId, string receiptToken, int seconds, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>Changes the invisibility of the message received from <see cref="ConsumeAsync"/>.</summary>
        Task<VisibilityResponse> ChangeVisibilityAsync(string topic, Message message, int seconds, CancellationToken cancellationToken = default(CancellationToken));

        /// <summary>
        /// The consume loop: long polling, calls <paramref name="handler"/> and acknowledges by itself only if
        /// it finishes without an exception. Ends, without an exception, when the token is cancelled.
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

        /// <summary>The synchronous form of <see cref="SubscribeAsync"/>: blocks the thread until cancellation.</summary>
        void Subscribe(string topic, ConsumeOptions options, Action<Message> handler, CancellationToken cancellationToken, Action<Message, Exception> onError = null);
    }
}
