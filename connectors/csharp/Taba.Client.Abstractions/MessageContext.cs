using System;
using System.Threading;
using System.Threading.Tasks;

namespace Taba
{
    /// <summary>
    /// O que o <c>handler</c> do <c>Consume</c> recebe: a <see cref="Message"/> com todas as propriedades,
    /// o <see cref="Delete"/> (a mensagem é apagada ao fim se o handler terminar sem exceção; o padrão é
    /// sim) e <see cref="ChangeVisibilityAsync"/>. Enquanto o handler roda, o conector renova sozinho a
    /// invisibilidade, para nenhum outro consumidor pegar a mensagem.
    /// </summary>
    public class MessageContext
    {
        private readonly Func<int, CancellationToken, Task> _change;
        private int _leaseSeconds;
        private int _lost;

        internal MessageContext(Message message, int leaseSeconds, Func<int, CancellationToken, Task> change, CancellationToken cancellationToken)
        {
            Message = message;
            _leaseSeconds = leaseSeconds;
            _change = change;
            CancellationToken = cancellationToken;
        }

        public Message Message { get; private set; }

        /// <summary>
        /// <c>true</c> (o padrão): a mensagem é apagada (ack) quando o handler termina sem exceção. Ponha
        /// <c>false</c> para deixá-la na fila: ela volta a ficar visível ao fim da invisibilidade.
        /// </summary>
        public bool Delete { get; set; } = true;

        /// <summary>O token do <c>Consume</c>, para o handler repassar às suas próprias chamadas.</summary>
        public CancellationToken CancellationToken { get; private set; }

        /// <summary>Os segundos de invisibilidade que a renovação automática pede a cada volta.</summary>
        public int LeaseSeconds
        {
            get { return Volatile.Read(ref _leaseSeconds); }
        }

        /// <summary><c>true</c> quando o servidor recusou a renovação (a mensagem já voltou à fila ou foi apagada).</summary>
        public bool LeaseLost
        {
            get { return Volatile.Read(ref _lost) != 0; }
        }

        /// <summary>
        /// Muda a invisibilidade para <paramref name="seconds"/> a partir de agora. O valor passa a ser o
        /// da renovação automática.
        /// </summary>
        public async Task ChangeVisibilityAsync(int seconds)
        {
            await _change(seconds, CancellationToken).ConfigureAwait(false);
            Volatile.Write(ref _leaseSeconds, seconds);
        }

        /// <summary>A forma síncrona do <see cref="ChangeVisibilityAsync"/>.</summary>
        public void ChangeVisibility(int seconds)
        {
            ChangeVisibilityAsync(seconds).GetAwaiter().GetResult();
        }

        internal void MarkLost()
        {
            Volatile.Write(ref _lost, 1);
        }
    }
}
