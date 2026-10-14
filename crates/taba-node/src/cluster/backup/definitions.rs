//! Definições de tópico e tokens no bucket (doc 23, "Backup em S3", seção 1.5).
//!
//! - **Tópico:** quem grava é o **nó gestor** (o dono da definição no anel, como em `09-topicos.md`),
//!   num `.json` por tópico: `topics/<nome>.json`. A remoção grava uma **lápide** no mesmo objeto e o
//!   prefixo das mensagens é apagado em segundo plano.
//! - **Token:** um `.json` por token, `tokens/<id>.json`, gravado pelo gestor dele (o dono da
//!   definição do token). A revogação apaga o objeto.
//!
//! Uma alteração de definição **nunca é recusada por falha no S3**: se a gravação falhar, a
//! alteração vale no cluster e entra numa fila de retentativa. Uma reconciliação periódica compara o
//! que o bucket guarda com a memória e regrava o que diverge.

use std::collections::HashMap;
use std::sync::{Arc, Weak};

use serde_json::{Value, json};

use super::{Backup, DefRef, lock};
use crate::cluster::protocol::{token_from_json, token_to_json};
use crate::core::token::TokenRecord;
use crate::core::topic::TopicDefinition;
use crate::engine::tokens::TokenEvent;
use crate::engine::topics::TopicEvent;
use crate::storage::backup::keys;

/// O que o gestor já gravou de cada definição (sem o instante), para não regravar o igual.
#[derive(Default)]
pub(crate) struct Written(HashMap<String, String>);

pub(crate) fn attach(backup: &Arc<Backup>) {
    let weak: Weak<Backup> = Arc::downgrade(backup);
    backup.topics.on_event(Arc::new({
        let weak = weak.clone();
        move |event| {
            if let Some(backup) = weak.upgrade() {
                backup.on_topic_event(event);
            }
        }
    }));
    backup.tokens.on_event(Arc::new(move |event| {
        if let Some(backup) = weak.upgrade() {
            backup.on_token_event(event);
        }
    }));
}

fn topic_body(definition: &TopicDefinition, now_ms: u64) -> Value {
    json!({ "kind": "topic", "savedMs": now_ms, "definition": definition.to_json() })
}

fn token_body(record: &TokenRecord, now_ms: u64) -> Value {
    json!({ "kind": "token", "savedMs": now_ms, "token": token_to_json(record) })
}

impl Backup {
    fn on_topic_event(&self, event: &TopicEvent) {
        match event {
            TopicEvent::Saved(definition) => {
                if self.topology.manages_definition("topic", &definition.name) {
                    lock(&self.runtime)
                        .pending_defs
                        .insert(DefRef::Topic(definition.name.clone()));
                }
            }
            TopicEvent::Deleted { name, id } => {
                // Todos os nós esquecem o que tinham do tópico; só o gestor limpa o bucket.
                let _ = self.db.drop_topic(name);
                if self.topology.manages_definition("topic", name) {
                    let mut runtime = lock(&self.runtime);
                    runtime.pending_defs.remove(&DefRef::Topic(name.clone()));
                    runtime
                        .pending_defs
                        .insert(DefRef::TopicDeleted(name.clone(), id.clone()));
                }
            }
        }
    }

    fn on_token_event(&self, event: &TokenEvent) {
        match event {
            TokenEvent::Saved(record) => {
                if self.topology.manages_definition("token", &record.id) {
                    lock(&self.runtime)
                        .pending_defs
                        .insert(DefRef::Token(record.id.clone()));
                }
            }
            TokenEvent::Revoked(id) => {
                if self.topology.manages_definition("token", id) {
                    let mut runtime = lock(&self.runtime);
                    runtime.pending_defs.remove(&DefRef::Token(id.clone()));
                    runtime
                        .pending_defs
                        .insert(DefRef::TokenRevoked(id.clone()));
                }
            }
        }
    }

    /// Grava no bucket as definições pendentes. As que falharem ficam para a próxima passada.
    pub(crate) async fn flush_definitions(&self) {
        if self.restoring.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        let pending: Vec<DefRef> = lock(&self.runtime).pending_defs.iter().cloned().collect();
        for def in pending {
            if self.write_definition(&def).await {
                lock(&self.runtime).pending_defs.remove(&def);
            }
        }
    }

    /// `true` se a definição ficou em dia no bucket.
    async fn write_definition(&self, def: &DefRef) -> bool {
        let now = self.clock.now_millis();
        match def {
            DefRef::Topic(name) => {
                let Some(definition) = self.topics.definition(name) else {
                    return true;
                };
                let key = keys::topic(name);
                let fingerprint = definition.to_json().to_string();
                self.put_if_changed(&key, &fingerprint, topic_body(&definition, now))
                    .await
            }
            DefRef::TopicDeleted(name, id) => {
                let body = json!({ "kind": "topic", "deleted": true, "id": id, "savedMs": now });
                let put = self
                    .store
                    .put(&keys::topic(name), body.to_string().into_bytes())
                    .await;
                self.note("PUT da lápide do tópico", &put);
                if put.is_err() {
                    return false;
                }
                lock(&self.written).0.remove(&keys::topic(name));
                lock(&self.runtime).purges.push((name.clone(), id.clone()));
                self.db.drop_topic(name).ok();
                true
            }
            DefRef::Token(id) => {
                let Some(record) = self.tokens.record(id) else {
                    return true;
                };
                let key = keys::token(id);
                let fingerprint = token_to_json(&record).to_string();
                self.put_if_changed(&key, &fingerprint, token_body(&record, now))
                    .await
            }
            DefRef::TokenRevoked(id) => {
                let deleted = self.store.delete(&keys::token(id)).await;
                self.note("DELETE do token", &deleted);
                if deleted.is_ok() {
                    lock(&self.written).0.remove(&keys::token(id));
                }
                deleted.is_ok()
            }
        }
    }

    /// Grava `body` em `key`, a menos que o bucket já tenha a mesma definição.
    async fn put_if_changed(&self, key: &str, fingerprint: &str, body: Value) -> bool {
        let known = lock(&self.written).0.get(key).cloned();
        let same = match known {
            Some(known) => known == fingerprint,
            // Um gestor novo (ou que acabou de subir) não sabe o que há no bucket: lê.
            None => match self.store.get(key).await {
                Ok(Some(object)) => {
                    bucket_fingerprint(&object.body).as_deref() == Some(fingerprint)
                }
                Ok(None) => false,
                Err(e) => {
                    self.note::<(), _>("GET da definição", &Err(e));
                    return false;
                }
            },
        };
        if same {
            lock(&self.written)
                .0
                .insert(key.to_owned(), fingerprint.to_owned());
            return true;
        }
        let put = self.store.put(key, body.to_string().into_bytes()).await;
        self.note("PUT da definição", &put);
        if put.is_err() {
            return false;
        }
        self.counters
            .definitions_written
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        lock(&self.written)
            .0
            .insert(key.to_owned(), fingerprint.to_owned());
        true
    }

    /// A reconciliação: toda definição de que este nó é o gestor e que o bucket não tem igual volta
    /// à fila de gravação.
    pub(crate) async fn reconcile_definitions(&self) {
        if !self.topology.is_active() || self.restoring.load(std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        {
            let mut runtime = lock(&self.runtime);
            for name in self.topics.names() {
                if self.topology.manages_definition("topic", &name) {
                    runtime.pending_defs.insert(DefRef::Topic(name));
                }
            }
            for id in self.tokens.ids() {
                if self.topology.manages_definition("token", &id) {
                    runtime.pending_defs.insert(DefRef::Token(id));
                }
            }
        }
        self.flush_definitions().await;
    }

    /// Apaga, em segundo plano, o prefixo das mensagens e dos diários de tópicos removidos.
    pub(crate) async fn purge_removed_topics(&self) {
        let pending: Vec<(String, String)> = std::mem::take(&mut lock(&self.runtime).purges);
        let mut failed = Vec::new();
        for (name, id) in pending {
            let mut ok = true;
            for prefix in [
                keys::segments_prefix(&name, &id),
                keys::journals_prefix(&name, &id),
            ] {
                let listed = self.store.list(&prefix).await;
                self.note("LIST do tópico removido", &listed);
                let Ok(objects) = listed else {
                    ok = false;
                    continue;
                };
                for object in objects {
                    let deleted = self.store.delete(&object.key).await;
                    self.note("DELETE do tópico removido", &deleted);
                    ok &= deleted.is_ok();
                }
            }
            if !ok {
                failed.push((name, id));
            }
        }
        if !failed.is_empty() {
            lock(&self.runtime).purges.extend(failed);
        }
    }
}

/// A definição que o objeto guarda, no mesmo formato da impressão digital (para comparar).
pub(crate) fn bucket_fingerprint(body: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if value.get("deleted").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    value
        .get("definition")
        .or_else(|| value.get("token"))
        .map(Value::to_string)
}

/// Lê um objeto de `topics/` como definição.
pub(crate) fn parse_topic(body: &[u8]) -> Option<TopicDefinition> {
    let value: Value = serde_json::from_slice(body).ok()?;
    if value.get("deleted").and_then(Value::as_bool) == Some(true) {
        return None;
    }
    TopicDefinition::from_json(value.get("definition")?).ok()
}

/// Lê um objeto de `tokens/` como token.
pub(crate) fn parse_token(body: &[u8]) -> Option<TokenRecord> {
    let value: Value = serde_json::from_slice(body).ok()?;
    token_from_json(value.get("token")?).ok()
}
