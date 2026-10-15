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
        /// <summary>
        /// Reads from the queue and hands each message to <paramref name="handler"/>, with the whole message, the
        /// <see cref="MessageContext.Delete"/> (default <c>true</c>: the message is deleted if the handler
        /// finishes without an exception) and <see cref="MessageContext.ChangeVisibilityAsync"/>. While the handler
        /// runs, a second task renews the invisibility every third of it (the
        /// <c>visibilityTimeout</c>, default 120 s; <c>maxMessages</c> default 1; <c>waitTime</c> default 20 s, the long polling), and stops by itself when it
        /// finishes. If the handler throws, the message is not deleted (it goes back to the queue at the end of the invisibility) and the
        /// exception propagates; the messages of the same batch not yet handled also come back by themselves. Returns
        /// how many messages were handled.
        /// </summary>
        public async Task<int> ConsumeAsync(string topic, ConsumeOptions options, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            options = WithHandlerDefaults(options);
            IList<Message> messages = await ConsumeAsync(topic, options, cancellationToken).ConfigureAwait(false);
            int handled = 0;
            foreach (Message message in messages)
            {
                MessageContext context = NewContext(topic, options, message, cancellationToken);
                await HandleAsync(topic, context, () => handler(context), cancellationToken).ConfigureAwait(false);
                handled++;
            }
            return handled;
        }

        /// <summary>
        /// The <see cref="ConsumeAsync(string, ConsumeOptions, Func{MessageContext, Task}, CancellationToken)"/>
        /// with the body already deserialized as <typeparamref name="T"/> (by the <see cref="TabaConfig.JsonSerializer"/>
        /// or the connector default) in <see cref="MessageContext{T}.Body"/>. If the body is not a
        /// <typeparamref name="T"/>, it throws <c>client.invalid-response</c> before calling the handler, and the message
        /// is not deleted.
        /// </summary>
        public async Task<int> ConsumeAsync<T>(string topic, ConsumeOptions options, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken))
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            options = WithHandlerDefaults(options);
            ITabaJsonSerializer serializer = _config.JsonSerializer ?? DefaultSerializer;
            IList<Message> messages = await ConsumeAsync(topic, options, cancellationToken).ConfigureAwait(false);
            int handled = 0;
            foreach (Message message in messages)
            {
                T body;
                try
                {
                    body = serializer.Deserialize<T>(message.BodyText);
                }
                catch (Exception e) when (!(e is TabaException))
                {
                    throw TabaException.Client("client.invalid-response", "body of message " + message.Id + " is not a " + typeof(T).Name, e);
                }
                var context = new MessageContext<T>(
                    message,
                    body,
                    options.VisibilityTimeout.Value,
                    (seconds, token) => ChangeVisibilityAsync(topic, message, seconds, token),
                    cancellationToken);
                await HandleAsync(topic, context, () => handler(context), cancellationToken).ConfigureAwait(false);
                handled++;
            }
            return handled;
        }

        /// <summary><c>ConsumeAsync&lt;T&gt;</c> without options: the server defaults apply.</summary>
        public Task<int> ConsumeAsync<T>(string topic, Func<MessageContext<T>, Task> handler, CancellationToken cancellationToken = default(CancellationToken))
        {
            return ConsumeAsync<T>(topic, null, handler, cancellationToken);
        }

        private MessageContext NewContext(string topic, ConsumeOptions options, Message message, CancellationToken cancellationToken)
        {
            return new MessageContext(
                message,
                options.VisibilityTimeout.Value,
                (seconds, token) => ChangeVisibilityAsync(topic, message, seconds, token),
                cancellationToken);
        }

        /// <summary>The <see cref="ConsumeAsync(string, ConsumeOptions, Func{MessageContext, Task}, CancellationToken)"/> without options: the server defaults apply.</summary>
        public Task<int> ConsumeAsync(string topic, Func<MessageContext, Task> handler, CancellationToken cancellationToken = default(CancellationToken))
        {
            return ConsumeAsync(topic, null, handler, cancellationToken);
        }

        private async Task HandleAsync(string topic, MessageContext context, Func<Task> run, CancellationToken cancellationToken)
        {
            Message message = context.Message;
            using (var stop = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken))
            {
                Task renewal = KeepAliveAsync(topic, context, stop.Token);
                try
                {
                    await run().ConfigureAwait(false);
                }
                finally
                {
                    stop.Cancel();
                    await renewal.ConfigureAwait(false);
                }
            }
            if (context.Delete)
            {
                try
                {
                    // Without the token: the message was already handled, and acknowledging it must not be cut short by cancellation.
                    await AckAsync(topic, message, CancellationToken.None).ConfigureAwait(false);
                }
                catch (TabaException e) when (e.Status == 409 || e.Status == 404)
                {
                    // The message already went back to the queue (deadline expired) or is already gone: there is nothing to acknowledge.
                }
            }
        }

        /// <summary>
        /// The defaults of <c>Consume</c> with a handler: one message at a time (<c>maxMessages</c> 1), 20 s long polling (<c>waitTime</c>) and 120 s of
        /// invisibility, which the renewal keeps while the handler runs. What the caller set applies.
        /// </summary>
        internal static ConsumeOptions WithHandlerDefaults(ConsumeOptions options)
        {
            ConsumeOptions copy = options == null ? new ConsumeOptions() : options.Clone();
            copy.MaxMessages = copy.MaxMessages ?? DefaultHandlerMaxMessages;
            copy.VisibilityTimeout = copy.VisibilityTimeout ?? DefaultHandlerVisibilityTimeout;
            copy.WaitTime = copy.WaitTime ?? DefaultHandlerWaitTime;
            return copy;
        }

        /// <summary>
        /// The renewal interval: a third of the invisibility (and never all of it), so there is time left
        /// to retry if a renewal fails and the message is not released to another consumer.
        /// </summary>
        internal static TimeSpan RenewalInterval(int leaseSeconds)
        {
            return TimeSpan.FromSeconds(Math.Max(1, leaseSeconds) / 3.0);
        }

        /// <summary>The second task: renews the invisibility until the handler finishes. Never throws.</summary>
        private async Task KeepAliveAsync(string topic, MessageContext context, CancellationToken token)
        {
            try
            {
                while (!token.IsCancellationRequested)
                {
                    await _transport.Delay(RenewalInterval(context.LeaseSeconds), token).ConfigureAwait(false);
                    try
                    {
                        await ChangeVisibilityAsync(topic, context.Message, context.LeaseSeconds, token).ConfigureAwait(false);
                    }
                    catch (TabaException e) when (e.Status == 409 || e.Status == 404)
                    {
                        // The message already went back to the queue or was deleted: renewing is no longer of any use.
                        context.MarkLost();
                        return;
                    }
                    catch (TabaException)
                    {
                        // Transient failure (network, 5xx): tries again on the next round, before the deadline expires.
                    }
                }
            }
            catch (OperationCanceledException)
            {
            }
        }

    }
}
