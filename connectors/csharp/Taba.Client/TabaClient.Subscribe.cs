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
        // subscribe
        // -----------------------------------------------------------------------------------------

        /// <summary>
        /// The consume loop (D71.1): long polling (<c>waitTime</c> 20 and one message at a time, if not
        /// set), calls <paramref name="handler"/> and acknowledges by itself **only if it finishes without an
        /// exception** (a message that fails goes back to the queue at the end of the <c>visibilityTimeout</c>, and the error goes
        /// to <paramref name="onError"/>). Ends, without an exception, when <paramref name="cancellationToken"/>
        /// is cancelled. A server error while consuming propagates.
        /// </summary>
        public async Task SubscribeAsync(
            string topic,
            ConsumeOptions options,
            Func<Message, Task> handler,
            CancellationToken cancellationToken,
            Action<Message, Exception> onError = null)
        {
            if (handler == null)
            {
                throw new ArgumentNullException("handler");
            }
            ConsumeOptions poll = options == null ? new ConsumeOptions() : options.Clone();
            if (!poll.WaitTime.HasValue)
            {
                poll.WaitTime = 20;
            }
            if (!poll.MaxMessages.HasValue)
            {
                poll.MaxMessages = 1;
            }
            while (!cancellationToken.IsCancellationRequested)
            {
                IList<Message> messages;
                try
                {
                    messages = await ConsumeAsync(topic, poll, cancellationToken).ConfigureAwait(false);
                }
                catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
                {
                    return;
                }
                foreach (Message message in messages)
                {
                    bool handled;
                    try
                    {
                        await handler(message).ConfigureAwait(false);
                        handled = true;
                    }
                    catch (Exception e)
                    {
                        handled = false;
                        if (onError != null)
                        {
                            onError(message, e);
                        }
                    }
                    if (handled)
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
                    if (cancellationToken.IsCancellationRequested)
                    {
                        return;
                    }
                }
            }
        }

    }
}
