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
        /// O laço de consumo (D71.1): long polling (<c>waitTime</c> 20 e uma mensagem por vez, se não
        /// informados), chama o <paramref name="handler"/> e confirma sozinho **só se ele terminar sem
        /// exceção** (a mensagem que falha volta à fila ao fim do <c>visibilityTimeout</c>, e o erro vai
        /// para <paramref name="onError"/>). Termina, sem exceção, quando o <paramref name="cancellationToken"/>
        /// é cancelado. Um erro do servidor ao consumir sobe.
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
                            // Sem o token: a mensagem já foi tratada, e confirmá-la não deve ser cortado pelo cancelamento.
                            await AckAsync(topic, message, CancellationToken.None).ConfigureAwait(false);
                        }
                        catch (TabaException e) when (e.Status == 409 || e.Status == 404)
                        {
                            // A mensagem já voltou à fila (prazo vencido) ou já foi embora: não há o que confirmar.
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
