# Pontos em aberto

[← Índice](README.md)

## 10. Pontos em aberto (a definir)
 
- **Linguagem e runtime de implementação.** Preferência pessoal avançada em C#, porém Rust parece mais adequado aos requisitos de performance, baixo overhead e concorrência. Decisão pendente.
- **Confirmar o quórum mínimo de votação (padrão: 3).** Validar se 3 é adequado em diferentes tamanhos de cluster ou se deve escalar com o número de nós.
- Número de tentativas de ping (retries) antes de abrir a votação de exclusão (seção 6.3) — valor configurável, mas o padrão ainda não foi definido.
- Duração padrão do período de freezing (seção 6.7) e comportamento exato durante ele (rejeição no bootstrap vs. lista de espera).
- Comportamento de retry após exclusão de um nó por falha no registro (seção 6.5): tentativa imediata, backoff, intervenção manual?
- Parâmetros de retry do registro e do commit de mensagem (tentativas, intervalo/backoff) e tempo de expiração de registros pendentes sem commit (seção 3.1).
- Formato/canal do evento de "falha de perda de mensagem" (seção 7.3): log, métrica, alerta ativo?
- Otimização futura da listagem de IDs por bloco na sincronização (seção 7.3) para blocos muito grandes (ex.: merkle tree, hashing incremental por faixa) em vez de listar todos os IDs a cada vez.
- **Algoritmo exato de priorização de memória entre tópicos** (seção 3.2): janela de tempo para calcular a taxa de leitura, política de despejo (LRU/LFU por tópico), tratamento de tópicos novos sem histórico.
- Onde fica o diretório raiz de disco (`${topico}/${id}.json`) — path configurável, relativo a quê (por nó? por bloco?) — e limite/rotina de limpeza dos arquivos em disco.
- Semântica de entrega: at-least-once, at-most-once ou exactly-once.
- Ordenação de mensagens (por tópico, por chave ou sem garantia).
- Modelo de consumo: polling, long polling ou push.
- Política de retenção, expiração (TTL) e dead-letter.
- Métricas e observabilidade: as tags de tópico já alimentam atributos OpenTelemetry (seção 8.1); falta definir o conjunto de métricas/traces exportados, o endpoint OTLP configurável e se haverá também endpoint Prometheus.
- Propagação de criação/alteração/remoção de tópico entre os nós do cluster (provavelmente pelo mesmo protocolo registro → commit) e o comportamento do `DELETE /topics/{topic}` com mensagens ainda pendentes.

