//! As métricas de nó que existem (doc 11, "Métricas de nó"), com o nome, o tipo e a unidade da
//! tabela do documento. O que ainda não existe (canal, votação, sincronização) não está aqui.

use super::{Def, Kind};

/// Os baldes sugeridos pela convenção semântica de HTTP (5 ms a 10 s).
const HTTP_SECONDS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.075, 0.1, 0.25, 0.5, 0.75, 1.0, 2.5, 5.0, 7.5, 10.0,
];
const BODY_BYTES: &[f64] = &[
    100.0,
    1_000.0,
    10_000.0,
    100_000.0,
    1_000_000.0,
    10_000_000.0,
];
const WAIT_SECONDS: &[f64] = &[
    0.001, 0.005, 0.01, 0.05, 0.1, 0.5, 1.0, 5.0, 10.0, 60.0, 300.0,
];
const NO_BUCKETS: &[f64] = &[];

const fn def(name: &'static str, kind: Kind, unit: &'static str, help: &'static str) -> Def {
    Def {
        name,
        kind,
        unit,
        help,
        buckets: NO_BUCKETS,
    }
}

const fn histogram(
    name: &'static str,
    unit: &'static str,
    help: &'static str,
    buckets: &'static [f64],
) -> Def {
    Def {
        name,
        kind: Kind::Histogram,
        unit,
        help,
        buckets,
    }
}

// ---- Processo e host ----

pub static PROCESS_UPTIME: Def = def(
    "process.uptime",
    Kind::Gauge,
    "s",
    "Tempo desde o início do processo.",
);
pub static PROCESS_CPU_UTILIZATION: Def = def(
    "process.cpu.utilization",
    Kind::Gauge,
    "1",
    "Uso de CPU do processo, de 0 a 1 do total de núcleos, desde a leitura anterior.",
);
pub static PROCESS_MEMORY_USAGE: Def = def(
    "process.memory.usage",
    Kind::UpDownCounter,
    "By",
    "Memória residente do processo.",
);
pub static PROCESS_THREAD_COUNT: Def = def(
    "process.thread.count",
    Kind::UpDownCounter,
    "{thread}",
    "Threads do sistema operacional do processo.",
);
pub static PROCESS_OPEN_FILES: Def = def(
    "process.open_file_descriptor.count",
    Kind::UpDownCounter,
    "{file}",
    "Descritores de arquivo abertos.",
);
pub static NODE_CONNECTIONS: Def = def(
    "taba.node.connections",
    Kind::UpDownCounter,
    "{connection}",
    "Conexões HTTP abertas, por porta.",
);

// ---- Armazenamento ----

pub static STORAGE_MEMORY_USAGE: Def = def(
    "taba.storage.memory.usage",
    Kind::UpDownCounter,
    "By",
    "Memória ocupada por corpos de mensagem.",
);
pub static STORAGE_MEMORY_LIMIT: Def = def(
    "taba.storage.memory.limit",
    Kind::Gauge,
    "By",
    "Limite de memória de mensagens (memory.limit).",
);
pub static STORAGE_MEMORY_MESSAGES: Def = def(
    "taba.storage.memory.messages",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens mantidas em memória.",
);
pub static STORAGE_DISK_MESSAGES: Def = def(
    "taba.storage.disk.messages",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens em disco.",
);
pub static STORAGE_DISK_FREE: Def = def(
    "taba.storage.disk.free",
    Kind::Gauge,
    "By",
    "Espaço livre no volume de storage.dir.",
);
pub static STORAGE_OVERFLOW: Def = def(
    "taba.storage.overflow",
    Kind::Counter,
    "{message}",
    "Mensagens que foram da memória para o disco por falta de espaço.",
);
pub static STORAGE_READS: Def = def(
    "taba.storage.reads",
    Kind::Counter,
    "{read}",
    "Leituras de mensagem, por onde foram atendidas (memória ou disco).",
);
pub static STORAGE_DISK_ERRORS: Def = def(
    "taba.storage.disk.errors",
    Kind::Counter,
    "{error}",
    "Falhas de entrada e saída em disco.",
);

// ---- API HTTP ----

pub static HTTP_SERVER_DURATION: Def = histogram(
    "http.server.request.duration",
    "s",
    "Latência das requisições recebidas; a contagem é o total de requisições.",
    HTTP_SECONDS,
);
pub static HTTP_SERVER_ACTIVE: Def = def(
    "http.server.active_requests",
    Kind::UpDownCounter,
    "{request}",
    "Requisições em andamento.",
);
pub static HTTP_SERVER_REQUEST_BODY: Def = histogram(
    "http.server.request.body.size",
    "By",
    "Tamanho do corpo das requisições (quando o tamanho é conhecido).",
    BODY_BYTES,
);
pub static HTTP_SERVER_RESPONSE_BODY: Def = histogram(
    "http.server.response.body.size",
    "By",
    "Tamanho do corpo das respostas (quando o tamanho é conhecido).",
    BODY_BYTES,
);
pub static RATELIMIT_REJECTED: Def = def(
    "taba.ratelimit.rejected",
    Kind::Counter,
    "{request}",
    "Requisições recusadas com 429.",
);
pub static AUTH_REQUESTS: Def = def(
    "taba.auth.requests",
    Kind::Counter,
    "{request}",
    "Autenticação por token: aceitas, recusadas com 401 e recusadas com 403.",
);
pub static AUTH_TOKENS: Def = def(
    "taba.auth.tokens",
    Kind::Gauge,
    "{token}",
    "Tokens de acesso que este nó conhece.",
);

// ---- Entrega ----

pub static DELIVERY_READY: Def = def(
    "taba.delivery.ready",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens prontas para entrega neste nó (todos os tópicos).",
);
pub static DELIVERY_FAIR_WAIT: Def = histogram(
    "taba.delivery.fair.wait",
    "s",
    "Tempo entre a publicação e a primeira entrega, por faixa do grupo.",
    WAIT_SECONDS,
);
pub static DELIVERY_REDELIVERIES: Def = def(
    "taba.delivery.redeliveries",
    Kind::Counter,
    "{message}",
    "Entregas de mensagens já entregues antes.",
);
pub static DELIVERY_DEAD_LETTERED: Def = def(
    "taba.delivery.dead_lettered",
    Kind::Counter,
    "{message}",
    "Mensagens enviadas à dead-letter por atingirem maxDeliveries.",
);
pub static RETENTION_EXPIRED: Def = def(
    "taba.retention.expired",
    Kind::Counter,
    "{message}",
    "Mensagens descartadas por ttl.",
);
pub static FIFO_DEDUPLICATED: Def = def(
    "taba.fifo.deduplicated",
    Kind::Counter,
    "{message}",
    "Publicações em fila FIFO descartadas por deduplicação.",
);
pub static FIFO_GROUPS_BUSY: Def = def(
    "taba.fifo.groups.busy",
    Kind::UpDownCounter,
    "{group}",
    "Grupos de filas FIFO com mensagem em voo neste nó.",
);
pub static LONGPOLL_WAITING: Def = def(
    "taba.delivery.longpoll.waiting",
    Kind::UpDownCounter,
    "{request}",
    "Chamadas de long polling aguardando neste nó.",
);
pub static LONGPOLL_OUTCOME: Def = def(
    "taba.delivery.longpoll.outcome",
    Kind::Counter,
    "{request}",
    "Como terminaram as chamadas de long polling.",
);

// ---- Estado do nó ----

pub static NODE_STATE: Def = def(
    "taba.node.state",
    Kind::Gauge,
    "{flag}",
    "1 no estado atual do nó.",
);
pub static NODE_DRAIN_STATE: Def = def(
    "taba.node.drain.state",
    Kind::Gauge,
    "{flag}",
    "1 no estado do drain do nó.",
);
pub static NODE_DRAIN_PENDING: Def = def(
    "taba.node.drain.pending",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens ainda em memória (chega a 0 quando o nó está drained).",
);

// ---- Cluster e anel (só em mode=production) ----

pub static CLUSTER_MEMBERS: Def = def(
    "taba.cluster.members",
    Kind::Gauge,
    "{node}",
    "Membros na tabela deste nó, por estado.",
);
pub static CLUSTER_MEMBERS_VERSION: Def = def(
    "taba.cluster.members.version",
    Kind::Gauge,
    "{version}",
    "Versão (época) da tabela de membros; divergência entre nós indica propagação incompleta.",
);
pub static CLUSTER_MEMBERSHIP_CHANGES: Def = def(
    "taba.cluster.membership.changes",
    Kind::Counter,
    "{change}",
    "Registros e desregistros de nós, por fase.",
);
pub static CLUSTER_BOOTSTRAP_FAILURES: Def = def(
    "taba.cluster.bootstrap.failures",
    Kind::Counter,
    "{failure}",
    "Falhas de bootstrap deste nó, por motivo.",
);
pub static RING_VIRTUAL_NODES: Def = def(
    "taba.ring.virtual_nodes",
    Kind::Gauge,
    "{vnode}",
    "Nós virtuais deste nó no anel (ring.virtual-nodes).",
);
pub static RING_BLOCKS: Def = def(
    "taba.ring.blocks",
    Kind::UpDownCounter,
    "{block}",
    "Blocos que o nó atende, por papel e integridade.",
);

// ---- Escrita, entrega e repasse entre nós (só em mode=production) ----

pub static WRITE_COORDINATED: Def = def(
    "taba.write.coordinated",
    Kind::Counter,
    "{message}",
    "Operações sobre mensagens coordenadas por este nó (publicação, invisibilidade, ack, dead-letter).",
);
pub static WRITE_RETRIES: Def = def(
    "taba.write.retries",
    Kind::Counter,
    "{retry}",
    "Retries por fase e por nó de destino.",
);
pub static WRITE_ROLLBACKS: Def = def(
    "taba.write.rollbacks",
    Kind::Counter,
    "{rollback}",
    "Rollbacks enviados por falha em fechar o quórum (fase 1).",
);
pub static WRITE_COMMIT_TIMEOUTS: Def = def(
    "taba.write.commit_timeouts",
    Kind::Counter,
    "{timeout}",
    "Vezes que write.commit.timeout esgotou sem commit confirmado numa réplica.",
);
pub static WRITE_RECEIVED: Def = def(
    "taba.write.received",
    Kind::Counter,
    "{message}",
    "Registros, commits e rollbacks recebidos.",
);
pub static WRITE_PENDING: Def = def(
    "taba.write.pending",
    Kind::UpDownCounter,
    "{message}",
    "Registros pendentes (sem commit).",
);
pub static WRITE_PENDING_EXPIRED: Def = def(
    "taba.write.pending.expired",
    Kind::Counter,
    "{message}",
    "Registros descartados por expiração (sem commit nem rollback).",
);
pub static DELIVERY_TAKE: Def = def(
    "taba.delivery.take",
    Kind::Counter,
    "{request}",
    "Pedidos de entrega a outro dono (take), enviados e recebidos.",
);
pub static CLUSTER_NOTIFICATIONS: Def = def(
    "taba.cluster.channel.notifications",
    Kind::Counter,
    "{message}",
    "Avisos de mensagem nova enviados e recebidos.",
);
pub static CLUSTER_FORWARDED: Def = def(
    "taba.cluster.forwarded",
    Kind::Counter,
    "{request}",
    "Chamadas de cliente repassadas a outro nó (drain), por resultado.",
);

// ---- Canal entre nós, votação e freezing (doc 04, seções 6.3 e 6.7; doc 13, seção 12.9) ----

pub static CHANNEL_CONNECTIONS: Def = def(
    "taba.cluster.channel.connections",
    Kind::UpDownCounter,
    "{connection}",
    "Canais abertos com outros nós (esperado: nós do cluster − 1).",
);
pub static CHANNEL_STATE: Def = def(
    "taba.cluster.channel.state",
    Kind::Gauge,
    "{flag}",
    "1 se o canal com o peer está conectado, 0 se caído.",
);
pub static CHANNEL_RTT: Def = histogram(
    "taba.cluster.channel.rtt",
    "s",
    "Latência da chamada do canal (sinal de vida e quadros), por peer.",
    WAIT_SECONDS,
);
pub static CHANNEL_RECONNECTS: Def = def(
    "taba.cluster.channel.reconnects",
    Kind::Counter,
    "{attempt}",
    "Tentativas de reconexão do canal e resultado.",
);
pub static CHANNEL_QUEUE_OUT: Def = def(
    "taba.cluster.channel.queue.out",
    Kind::UpDownCounter,
    "{signal}",
    "Quadros na fila de saída para o peer.",
);
pub static CHANNEL_QUEUE_DROPPED: Def = def(
    "taba.cluster.channel.queue.dropped",
    Kind::Counter,
    "{signal}",
    "Quadros substituídos, descartados por idade ou por estouro da fila de saída.",
);
pub static CLUSTER_VOTE: Def = def(
    "taba.cluster.vote",
    Kind::Counter,
    "{vote}",
    "Votações de exclusão abertas ou votadas e o resultado.",
);
pub static CLUSTER_FREEZING: Def = def(
    "taba.cluster.freezing",
    Kind::Gauge,
    "{node}",
    "Nós atualmente em período de freezing.",
);
pub static CLUSTER_CONDITION: Def = def(
    "taba.cluster.condition",
    Kind::Gauge,
    "{flag}",
    "1 para cada condição vigente do cluster.",
);

// ---- Sincronização de blocos (doc 05, seção 7.3) ----

pub static SYNC_BLOCKS_ACTIVE: Def = def(
    "taba.sync.blocks.active",
    Kind::UpDownCounter,
    "{block}",
    "Blocos em sincronização agora (entrega suspensa nesses blocos).",
);
pub static SYNC_DURATION: Def = histogram(
    "taba.sync.duration",
    "s",
    "Tempo para tornar um bloco íntegro.",
    WAIT_SECONDS,
);
pub static SYNC_IDS_LISTED: Def = def(
    "taba.sync.ids.listed",
    Kind::Counter,
    "{id}",
    "IDs consolidados (união) nas listas de origem.",
);
pub static SYNC_MESSAGES_PULLED: Def = def(
    "taba.sync.messages.pulled",
    Kind::Counter,
    "{message}",
    "Mensagens obtidas por nó de origem.",
);
pub static SYNC_BYTES: Def = def(
    "taba.sync.bytes",
    Kind::Counter,
    "By",
    "Volume de corpo transferido na sincronização.",
);
pub static SYNC_LOG_EVENTS: Def = def(
    "taba.sync.log.events",
    Kind::UpDownCounter,
    "{event}",
    "Eventos no banco de eventos (sync.db) ainda por processar.",
);
pub static SYNC_LOG_BYTES: Def = def(
    "taba.sync.log.bytes",
    Kind::Gauge,
    "By",
    "Tamanho do banco de eventos em disco.",
);
pub static SYNC_PHASE: Def = def(
    "taba.sync.phase",
    Kind::Gauge,
    "{flag}",
    "1 na fase atual da sincronização do nó.",
);
pub static SYNC_FAILURES: Def = def(
    "taba.sync.failures",
    Kind::Counter,
    "{failure}",
    "Falhas de sincronização.",
);

// ---- Métricas de tópico (só nos modos by-tags e per-topic) ----

pub static TOPIC_PUBLISHED: Def = def(
    "taba.topic.published",
    Kind::Counter,
    "{message}",
    "Mensagens publicadas no tópico.",
);
pub static TOPIC_CONSUMED: Def = def(
    "taba.topic.consumed",
    Kind::Counter,
    "{message}",
    "Mensagens entregues em GET.",
);
pub static TOPIC_ACKED: Def = def(
    "taba.topic.acked",
    Kind::Counter,
    "{message}",
    "Confirmações (ack) do tópico.",
);
pub static TOPIC_VISIBLE: Def = def(
    "taba.topic.visible",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens prontas para entrega.",
);
pub static TOPIC_INVISIBLE: Def = def(
    "taba.topic.invisible",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens em processamento.",
);
pub static TOPIC_DEPTH: Def = def(
    "taba.topic.depth",
    Kind::UpDownCounter,
    "{message}",
    "Mensagens ainda não confirmadas.",
);
pub static TOPIC_OLDEST_TIMESTAMP: Def = def(
    "taba.topic.oldest_timestamp",
    Kind::Gauge,
    "s",
    "Segundos Unix da mensagem não confirmada mais antiga.",
);
pub static TOPIC_MEMORY_BYTES: Def = def(
    "taba.topic.memory_bytes",
    Kind::UpDownCounter,
    "By",
    "Bytes do tópico em memória.",
);
pub static TOPIC_DISK_BYTES: Def = def(
    "taba.topic.disk_bytes",
    Kind::UpDownCounter,
    "By",
    "Bytes do tópico em disco.",
);
pub static TOPIC_CONDITION: Def = def(
    "taba.topic.condition",
    Kind::Gauge,
    "{flag}",
    "1 para cada condição vigente do tópico.",
);

/// As métricas de tópico, para apagá-las juntas.
pub static TOPIC_DEFS: [&Def; 10] = [
    &TOPIC_PUBLISHED,
    &TOPIC_CONSUMED,
    &TOPIC_ACKED,
    &TOPIC_VISIBLE,
    &TOPIC_INVISIBLE,
    &TOPIC_DEPTH,
    &TOPIC_OLDEST_TIMESTAMP,
    &TOPIC_MEMORY_BYTES,
    &TOPIC_DISK_BYTES,
    &TOPIC_CONDITION,
];
