//! O lado que **serve** a sincronização de blocos (doc 06c, "Sincronização e recuperação"): os
//! streams de IDs e de mensagens de um bloco e a mensagem avulsa. É leitura local, sem estado
//! pendente. O nó que ainda sincroniza não serve (`503 cluster.node-syncing`).

use std::sync::Arc;

use serde_json::{Value, json};

use super::SyncRules;
use crate::cluster::protocol::{MessageRecord, Request, Response, StreamPage, StreamQuery};
use crate::cluster::service::ClusterService;
use crate::cluster::state::NodeState;
use crate::core::error::{ErrorCode, TabaError};
use crate::core::ring::BlockId;
use crate::core::time;
use crate::engine::messages::Messages;

pub struct BlockSource {
    service: Arc<ClusterService>,
    messages: Arc<Messages>,
    rules: SyncRules,
}

impl BlockSource {
    pub fn new(service: Arc<ClusterService>, messages: Arc<Messages>, rules: SyncRules) -> Self {
        Self {
            service,
            messages,
            rules,
        }
    }

    /// As faixas de posição do bloco no anel deste nó. Bloco que o anel não tem é uma tabela de
    /// membros diferente da de quem pediu: `409`, e quem pediu tenta de novo.
    fn ranges(&self, block: &str) -> Result<Vec<(u64, u64)>, TabaError> {
        let id: BlockId = block.parse().map_err(|()| {
            TabaError::new(
                ErrorCode::ValidationFailed,
                "O identificador do bloco deve ter 16 dígitos hexadecimais minúsculos.",
            )
        })?;
        self.service
            .membership()
            .ring()
            .block_ranges(id)
            .ok_or_else(|| {
                TabaError::new(
                    ErrorCode::ClusterConflict,
                    "O bloco não existe no anel deste nó: a tabela de membros é outra.",
                )
            })
    }

    fn page_size(&self, query: &StreamQuery) -> usize {
        query.limit.map_or(self.rules.max_items, |n| {
            (n as usize).clamp(2, self.rules.max_items)
        })
    }

    pub async fn handle(&self, request: Request) -> Result<Response, TabaError> {
        if self.service.node_state() != NodeState::Active {
            return Err(TabaError::new(
                ErrorCode::ClusterNodeSyncing,
                "O nó ainda está em sincronização e não serve blocos.",
            )
            .with_retry_after(5));
        }
        let messages = self.messages.clone();
        match request {
            Request::BlockIds { block, query } => {
                let ranges = self.ranges(&block)?;
                let limit = self.page_size(&query);
                let entries = tokio::task::spawn_blocking(move || {
                    messages.block_entries(
                        &ranges,
                        query.from.as_deref(),
                        query.to.as_deref(),
                        limit + 1,
                    )
                })
                .await
                .map_err(|_| TabaError::internal())??;
                let more = entries.len() > limit;
                let items: Vec<Value> = entries
                    .into_iter()
                    .take(limit)
                    .map(|entry| {
                        let mut line = json!({ "id": entry.id, "topic": entry.topic });
                        if let (Some(at), Some(object)) = (entry.delete_at_ms, line.as_object_mut())
                        {
                            object.insert("deleteAt".into(), json!(time::format_ms(at)));
                        }
                        line
                    })
                    .collect();
                Ok(Response::Stream(StreamPage::new(items, more)))
            }
            Request::BlockMessages { block, query } => {
                let ranges = self.ranges(&block)?;
                let (limit, max_bytes) = (self.page_size(&query), self.rules.max_bytes);
                let (found, more) = tokio::task::spawn_blocking(move || {
                    messages.block_messages(
                        &ranges,
                        query.from.as_deref(),
                        query.to.as_deref(),
                        limit,
                        max_bytes,
                    )
                })
                .await
                .map_err(|_| TabaError::internal())??;
                let items: Vec<Value> = found
                    .into_iter()
                    .map(|m| {
                        let mut record = MessageRecord::from_stored(&m.topic, m.meta, m.body);
                        record.delete_at_ms = m.delete_at_ms;
                        serde_json::to_value(record).unwrap_or(Value::Null)
                    })
                    .collect();
                Ok(Response::Stream(StreamPage::new(items, more)))
            }
            Request::BlockMessage { block, id } => {
                let ranges = self.ranges(&block)?;
                let found =
                    tokio::task::spawn_blocking(move || messages.block_message(&ranges, None, &id))
                        .await
                        .map_err(|_| TabaError::internal())??;
                Ok(Response::Record(MessageRecord::from_stored(
                    &found.topic,
                    found.meta,
                    found.body,
                )))
            }
            _ => Err(TabaError::route_not_found()),
        }
    }
}
