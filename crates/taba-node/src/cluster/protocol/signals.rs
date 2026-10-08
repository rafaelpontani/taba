//! Os quadros do canal entre nós (doc 13, seções 12.8 e 12.9; doc 06c, `GET /cluster/channel`) e o
//! repasse de uma chamada de cliente (`* /cluster/forward/{caminho}`).
//!
//! Enquanto o canal WebSocket não existe, o canal viaja como chamadas do `Transport`
//! (`POST /cluster/channel`): cada chamada leva o sinal de vida e os quadros ainda sem confirmação,
//! e a resposta é o `confirm`. A fila de saída, o `seq` e o reenvio são os do canal.

use serde::{Deserialize, Serialize};

/// Um quadro do canal (`type` e os campos do doc 06c).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "type",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum Signal {
    /// O tópico passou a ter (ou deixou de ter) ouvintes de long polling no nó que envia.
    Listeners { topic: String, has_listeners: bool },
    /// O nó que envia passou a ter (ou deixou de ter) mensagens prontas do tópico.
    Ready { topic: String, has_ready: bool },
    /// Aviso de mensagem nova: há `count` prontas; venha buscar com `take`.
    Notify { topic: String, count: u64 },
    /// O estado do nó: `draining` (os pares deixam de pedir `take`), `active` (ficou pronto ou
    /// cancelou o drain) ou `failed` (os outros ficam liberados para votar a exclusão).
    NodeState { state: String },
}

/// Um sinal com quem o enviou.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SignalFrame {
    /// O `nodeId` de quem envia.
    pub from: String,
    #[serde(flatten)]
    pub signal: Signal,
}

/// Quantas mensagens prontas um tópico tem no nó que envia o sinal de vida.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReadyCount {
    pub topic: String,
    pub count: u64,
}

/// O sinal de vida (`keepalive`, doc 06c): sem `seq`, não é confirmado nem reenviado, o seguinte o
/// substitui. Leva o estado do nó e a versão da tabela de membros (a época), o que as condições do
/// cluster comparam.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Keepalive {
    /// A data e hora do nó que envia.
    pub at: String,
    /// O indicador de carga: chamadas de cliente repassadas em andamento.
    pub load: u64,
    /// A versão (época) da tabela de membros do nó, ou `0` sem tabela.
    pub version: u64,
    /// `active`, `syncing`, `draining`, `failed`...
    pub state: String,
    /// Só os tópicos com mensagens prontas, até `cluster.channel.keepalive.max-topics`.
    pub ready: Vec<ReadyCount>,
}

/// Um quadro com `seq`: o emissor só o tira da fila de saída ao receber o `confirm`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelFrame {
    pub seq: u64,
    #[serde(flatten)]
    pub signal: Signal,
}

/// O que uma chamada ao canal leva: o sinal de vida (quando é hora) e os quadros pendentes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChannelBatch {
    /// O `nodeId` de quem envia.
    pub from: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive: Option<Keepalive>,
    #[serde(default)]
    pub frames: Vec<ChannelFrame>,
}

/// O `confirm`: os `seq` já **processados** (inclusive os que já tinham sido, num reenvio).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Confirm {
    #[serde(rename = "type")]
    pub kind: String,
    pub seqs: Vec<u64>,
}

impl Confirm {
    pub fn new(seqs: Vec<u64>) -> Self {
        Self {
            kind: "confirm".to_owned(),
            seqs,
        }
    }
}

/// Uma chamada de cliente repassada a outro nó: método, caminho com a query, cabeçalhos e corpo
/// como vieram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardRequest {
    pub method: String,
    /// O caminho original da API de cliente, com a query (`/topics/pedidos/messages?waitTime=5`).
    pub path_and_query: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    /// O IP do cliente já resolvido por quem repassa (o destino não conta o rate limit de novo).
    pub client_ip: Option<String>,
}

/// A resposta de uma chamada de cliente repassada, como o destino a deu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}
