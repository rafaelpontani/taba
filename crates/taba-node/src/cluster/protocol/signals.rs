//! Os sinais pequenos entre nós (doc 13, seção 12.8; doc 06c, `GET /cluster/channel`) e o repasse
//! de uma chamada de cliente (`* /cluster/forward/{caminho}`).
//!
//! Enquanto o canal WebSocket não existe, os sinais viajam como chamadas comuns do `Transport`
//! (`POST /cluster/signals`), com os mesmos quadros JSON do canal, sem `seq`, confirmação nem fila
//! de saída: um sinal perdido custa só uma dica velha (o `take` confere).

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
    /// O nó entrou em drain: os pares deixam de pedir `take` a ele.
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
