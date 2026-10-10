//! Os quadros do canal entre nós (doc 13, seções 12.8 e 12.9; doc 06c, `GET /cluster/channel`) e o
//! repasse de uma chamada de cliente (`* /cluster/forward/{caminho}`).
//!
//! O canal é uma conexão WebSocket por par de nós (`GET /cluster/channel`), e cada mensagem de
//! texto é um [`ChannelMessage`]: um lote (o sinal de vida e os quadros ainda sem confirmação) ou o
//! `confirm` do que o outro lado processou. A fila de saída, o `seq` e o reenvio são os do canal.

use serde::{Deserialize, Serialize};

use crate::cluster::members::Change;
use crate::cluster::protocol::DefinitionKind;

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
    /// O nó `node_id` foi excluído por votação e está em freezing desde `frozen_at` (ms desde 1970;
    /// doc 04, seção 6.7). Quem ainda não o conhecia o guarda e o repassa; não mexe na tabela.
    Frozen { node_id: String, frozen_at: u64 },
    /// Uma definição de tópico ou de token, no estado que o nó que envia a aplicou (`version`, `deleted`
    /// e o corpo): quem está atrás a aplica pela mesma regra do `definition-apply`. Enfileirado pelo
    /// dono e pelo herdeiro; o quadro novo da mesma definição substitui o antigo na fila (D127).
    Definition {
        kind: DefinitionKind,
        key: String,
        version: u64,
        deleted: bool,
        body: serde_json::Value,
    },
    /// Uma mudança de membros já efetivada (entrada, saída ou exclusão), com a versão que a tabela
    /// passou a ter e o `operationId` dela (doc 04, seção 6.2). Quem perdeu o registro ou o commit a
    /// aplica ao receber, sem depender da mudança pendente; o quadro fica na fila de saída até o
    /// `confirm` e nunca é substituído nem expira (D126).
    Membership {
        operation_id: String,
        version: u64,
        change: Change,
    },
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
    /// O `nodeId` de quem deve receber. Um processo novo no mesmo IP e porta (outro ID) responde
    /// `421` `cluster.wrong-node` em vez de passar pelo nó antigo (D100). Vazio: não confere.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub to: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub keepalive: Option<Keepalive>,
    #[serde(default)]
    pub frames: Vec<ChannelFrame>,
}

/// O que viaja pelo canal, em mensagens de texto JSON: o lote de quem envia ou o `confirm` de quem
/// recebe. O `confirm` se reconhece pelo `"type":"confirm"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelMessage {
    Batch(ChannelBatch),
    Confirm(Confirm),
}

impl ChannelMessage {
    pub fn to_text(&self) -> String {
        match self {
            ChannelMessage::Batch(batch) => serde_json::to_string(batch),
            ChannelMessage::Confirm(confirm) => serde_json::to_string(confirm),
        }
        .expect("o lote do canal sempre vira JSON")
    }

    pub fn from_text(text: &str) -> Result<Self, String> {
        let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if value.get("type").and_then(serde_json::Value::as_str) == Some("confirm") {
            serde_json::from_value(value)
                .map(ChannelMessage::Confirm)
                .map_err(|e| e.to_string())
        } else {
            serde_json::from_value(value)
                .map(ChannelMessage::Batch)
                .map_err(|e| e.to_string())
        }
    }
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
