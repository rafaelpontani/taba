# NATS JetStream

Imagem `nats:2.10.22-alpine`, um nó, fluxo em arquivo. `default` usa o `sync_interval` padrão; `fsync` usa
`sync_interval: always` (`nats-fsync.conf`).

Com `sync_interval: always` o NATS faz um fsync por mensagem, sem agrupar. Na taxa máxima as publicações estouram o
tempo de resposta do cliente (`response not registered in time`) e o controlador de taxa do OMB derruba a taxa a
zero, então o `max-rate-1kb` não mede nada. Por isso `max-rate-sweep` troca o `max-rate-1kb` do NATS por taxas fixas
(`rate-<n>-1kb`, o `simple-1kb` com a taxa `n`), e o relatório mostra a maior que ele sustenta com a mesma garantia
dos outros sistemas. Na rodada de 2026-11-03 ele não sustentou nem 500 por segundo: cada fsync do disco do Docker
Desktop leva cerca de 7 ms, o servidor avisa `Readloop processing time: 2s` e o cliente desiste depois de 2 s. Os
gráficos o deixam fora das barras e dizem na nota até onde ele chegou.
