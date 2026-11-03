# Kafka

Imagem `apache/kafka:3.8.0`, KRaft, um nó, fator 1. Driver `kafka` do OMB com `acks=all`.

- `default`: sem fsync por mensagem.
- `fsync`: `flush.messages=1` e `flush.ms=0` no tópico.
