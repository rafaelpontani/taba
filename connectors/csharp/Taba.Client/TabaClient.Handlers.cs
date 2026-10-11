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
        /// Lê da fila e entrega cada mensagem ao <paramref name="handler"/>, com a mensagem inteira, o
        /// <see cref="MessageContext.Delete"/> (padrão <c>true</c>: a mensagem é apagada se o handler
        /// terminar sem exceção) e <see cref="MessageContext.ChangeVisibilityAsync"/>. Enquanto o handler
        /// roda, uma segunda tarefa renova a invisibilidade a cada um terço dela (o
        /// <c>visibilityTimeout</c>, padrão 120 s; <c>maxMessages</c> padrão 1; <c>waitTime</c> padrão 20 s, o long polling), e para sozinha quando ele
        /// termina. Se o handler lança, a mensagem não é apagada (volta à fila ao fim da invisibilidade) e a
        /// exceção sobe; as mensagens ainda não tratadas do mesmo lote também voltam sozinhas. Devolve
        /// quantas mensagens foram tratadas.
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
        /// O <see cref="ConsumeAsync(string, ConsumeOptions, Func{MessageContext, Task}, CancellationToken)"/>
        /// com o corpo já deserializado como <typeparamref name="T"/> (pelo <see cref="TabaConfig.JsonSerializer"/>
        /// ou o padrão do conector) em <see cref="MessageContext{T}.Body"/>. Se o corpo não é um
        /// <typeparamref name="T"/>, lança <c>client.invalid-response</c> antes de chamar o handler, e a mensagem
        /// não é apagada.
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
                    throw TabaException.Client("client.invalid-response", "corpo da mensagem " + message.Id + " não é um " + typeof(T).Name, e);
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

        /// <summary>O <c>ConsumeAsync&lt;T&gt;</c> sem opções: valem os padrões do servidor.</summary>
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

        /// <summary>O <see cref="ConsumeAsync(string, ConsumeOptions, Func{MessageContext, Task}, CancellationToken)"/> sem opções: valem os padrões do servidor.</summary>
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
                    // Sem o token: a mensagem já foi tratada, e confirmá-la não deve ser cortada pelo cancelamento.
                    await AckAsync(topic, message, CancellationToken.None).ConfigureAwait(false);
                }
                catch (TabaException e) when (e.Status == 409 || e.Status == 404)
                {
                    // A mensagem já voltou à fila (prazo vencido) ou já foi embora: não há o que confirmar.
                }
            }
        }

        /// <summary>
        /// Os padrões do <c>Consume</c> com handler: uma mensagem por vez (<c>maxMessages</c> 1), long polling de 20 s (<c>waitTime</c>) e 120 s de
        /// invisibilidade, que a renovação mantém enquanto o handler roda. O que a pessoa informou vale.
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
        /// O intervalo da renovação: um terço da invisibilidade (e nunca ela inteira), para sobrar tempo
        /// de repetir se uma renovação falhar e a mensagem não ser liberada a outro consumidor.
        /// </summary>
        internal static TimeSpan RenewalInterval(int leaseSeconds)
        {
            return TimeSpan.FromSeconds(Math.Max(1, leaseSeconds) / 3.0);
        }

        /// <summary>A segunda tarefa: renova a invisibilidade até o handler terminar. Nunca lança.</summary>
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
                        // A mensagem já voltou à fila ou foi apagada: renovar não adianta mais.
                        context.MarkLost();
                        return;
                    }
                    catch (TabaException)
                    {
                        // Falha passageira (rede, 5xx): tenta de novo na próxima volta, antes de vencer o prazo.
                    }
                }
            }
            catch (OperationCanceledException)
            {
            }
        }

    }
}
