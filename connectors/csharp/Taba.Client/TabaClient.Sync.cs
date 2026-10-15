using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    public sealed partial class TabaClient
    {
        // -----------------------------------------------------------------------------------------
        // The synchronous form: waits for the asynchronous one
        // -----------------------------------------------------------------------------------------

        public Topic CreateTopic(TopicRequest request)
        {
            return Wait(CreateTopicAsync(request));
        }

        public Topic GetTopic(string name)
        {
            return Wait(GetTopicAsync(name));
        }

        public Page<Topic> ListTopics(int? limit = null, string cursor = null, string name = null)
        {
            return Wait(ListTopicsAsync(limit, cursor, name));
        }

        public IList<Topic> ListAllTopics(string name = null)
        {
            return Wait(ListAllTopicsAsync(name));
        }

        public Topic UpdateTopic(string name, TopicUpdate update)
        {
            return Wait(UpdateTopicAsync(name, update));
        }

        public void DeleteTopic(string name)
        {
            Wait(DeleteTopicAsync(name));
        }

        public PublishResponse Publish(string topic, byte[] body, PublishOptions options = null)
        {
            return Wait(PublishAsync(topic, body, options));
        }

        public PublishResponse PublishJson<T>(string topic, T value)
        {
            return Wait(PublishJsonAsync(topic, value));
        }

        public PublishResponse PublishRawJson(string topic, string json)
        {
            return Wait(PublishRawJsonAsync(topic, json));
        }

        public PublishResponse PublishText(string topic, string text)
        {
            return Wait(PublishTextAsync(topic, text));
        }

        public IList<Message> Consume(string topic, ConsumeOptions options = null)
        {
            return Wait(ConsumeAsync(topic, options));
        }

        /// <summary>The synchronous form of <c>ConsumeAsync</c> with a handler.</summary>
        public int Consume(string topic, ConsumeOptions options, Action<MessageContext> handler)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            return Wait(ConsumeAsync(
                topic,
                options,
                context =>
                {
                    handler(context);
                    return Task.FromResult(0);
                }));
        }

        /// <summary>The synchronous form of <c>ConsumeAsync</c> with a handler, without options.</summary>
        public int Consume(string topic, Action<MessageContext> handler)
        {
            return Consume(topic, null, handler);
        }

        /// <summary>The synchronous form of <c>ConsumeAsync&lt;T&gt;</c>.</summary>
        public int Consume<T>(string topic, ConsumeOptions options, Action<MessageContext<T>> handler)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            return Wait(ConsumeAsync<T>(
                topic,
                options,
                context =>
                {
                    handler(context);
                    return Task.FromResult(0);
                }));
        }

        public int Consume<T>(string topic, Action<MessageContext<T>> handler)
        {
            return Consume<T>(topic, null, handler);
        }

        public void Ack(string topic, string messageId, string receiptToken)
        {
            Wait(AckAsync(topic, messageId, receiptToken));
        }

        public void Ack(string topic, Message message)
        {
            Wait(AckAsync(topic, message));
        }

        public VisibilityResponse ChangeVisibility(string topic, string messageId, string receiptToken, int seconds)
        {
            return Wait(ChangeVisibilityAsync(topic, messageId, receiptToken, seconds));
        }

        public VisibilityResponse ChangeVisibility(string topic, Message message, int seconds)
        {
            return Wait(ChangeVisibilityAsync(topic, message, seconds));
        }

        /// <summary>The synchronous form of <see cref="SubscribeAsync"/>: blocks the thread until cancellation.</summary>
        public void Subscribe(string topic, ConsumeOptions options, Action<Message> handler, CancellationToken cancellationToken, Action<Message, Exception> onError = null)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            Wait(SubscribeAsync(
                topic,
                options,
                message =>
                {
                    handler(message);
                    return Task.FromResult(0);
                },
                cancellationToken,
                onError));
        }

        private static void Wait(Task task)
        {
            try
            {
                task.GetAwaiter().GetResult();
            }
            catch (AggregateException e) when (e.InnerExceptions.Count == 1)
            {
                throw e.InnerExceptions[0];
            }
        }

        private static T Wait<T>(Task<T> task)
        {
            Wait((Task)task);
            return task.GetAwaiter().GetResult();
        }

    }
}
