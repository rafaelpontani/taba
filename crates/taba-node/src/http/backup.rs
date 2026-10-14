//! As rotas de operação do backup em S3 (doc 23, "Backup em S3", seção 1.11), na API de operação
//! (como `/node/drain`: sem token, só os IPs de `operations.allowed-ips`, categoria `admin` do rate
//! limit).
//!
//! | Rota | O que faz |
//! |---|---|
//! | `GET /node/backup` | O estado do backup neste nó (fila de envio, S3 no ar, segmentos) |
//! | `POST /node/backup/flush` | Força o backup neste nó: memória → disco, disco → S3, `outbox` → S3. Não desregistra nem para o nó |
//! | `POST /cluster/backup/flush` | O nó que recebe repassa a todos os outros e junta as respostas |
//! | `POST /cluster/stop` | Drain em **todos** os nós e a lista dos nós vivos na parada |
//!
//! "Iniciar o cluster" não é uma rota: é subir os processos (`docker compose start`); a restauração
//! acontece sozinha na partida.

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::{Value, json};

use super::AppState;
use super::common::ApiError;
use crate::cluster::backup::{Backup, BackupStatus, not_configured};
use crate::cluster::protocol::{Request as PeerRequest, Response as PeerResponse};
use crate::core::time::format_ms;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/node/backup", get(status))
        .route("/node/backup/flush", post(node_flush))
        .route("/cluster/backup/flush", post(cluster_flush))
        .route("/cluster/stop", post(cluster_stop))
}

fn backup_of(state: &AppState) -> Result<&Arc<Backup>, ApiError> {
    state.backup.as_ref().ok_or_else(|| not_configured().into())
}

/// O `BackupStatus` como a API o mostra.
pub fn status_json(status: &BackupStatus) -> Value {
    json!({
        "s3Reachable": status.s3_ok,
        "lastError": status.last_error,
        "lastSuccessAt": status.last_ok_ms.map(format_ms),
        "outboxPending": status.outbox_pending,
        "journalPending": status.journal_pending,
        "definitionsPending": status.definitions_pending,
        "pending": status.pending(),
        "segments": status.segments,
        "segmentBytes": status.segment_bytes,
        "saved": status.saved,
        "segmentsUploaded": status.segments_uploaded,
        "messagesUploaded": status.messages_uploaded,
        "bodiesEvicted": status.bodies_evicted,
        "errors": status.errors,
    })
}

async fn status(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    Ok(Json(status_json(&backup.status())))
}

async fn node_flush(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    let report = backup.flush().await;
    let mut body = report.to_json();
    body["nodeId"] = json!(backup.node_id());
    Ok(Json(body))
}

/// Chama `request` em cada membro (a si mesmo sem rede), todos ao mesmo tempo, e devolve as
/// respostas na ordem em que chegaram: `(nodeId, Ok(json) | Err(motivo))`.
async fn ask_all(state: &AppState, request: PeerRequest) -> Vec<(String, Result<Value, String>)> {
    let Some(cluster) = &state.cluster else {
        return Vec::new();
    };
    let Some(table) = cluster.membership().table() else {
        return Vec::new();
    };
    let mut set = tokio::task::JoinSet::new();
    for member in table.members {
        let (cluster, request) = (cluster.clone(), request.clone());
        set.spawn(async move {
            let answer = match cluster.call_member(&member, request).await {
                Ok(PeerResponse::Backup(report)) => Ok(report),
                Ok(_) => Err("resposta inesperada".to_owned()),
                Err(e) => Err(e.to_string()),
            };
            (member.node_id, answer)
        });
    }
    let mut answers = Vec::new();
    while let Some(Ok(answer)) = set.join_next().await {
        answers.push(answer);
    }
    answers
}

async fn cluster_flush(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = backup_of(&state)?;
    if state.cluster.is_none() {
        let mut body = backup.flush().await.to_json();
        body["nodeId"] = json!(backup.node_id());
        return Ok(Json(
            json!({ "complete": body["complete"], "nodes": [body] }),
        ));
    }
    let mut answers = ask_all(&state, PeerRequest::BackupFlush).await;
    answers.sort_by(|a, b| a.0.cmp(&b.0));
    let mut complete = true;
    let nodes: Vec<Value> = answers
        .into_iter()
        .map(|(node_id, answer)| match answer {
            Ok(mut report) => {
                complete &= report["complete"].as_bool().unwrap_or(false);
                report["nodeId"] = json!(node_id);
                report
            }
            // O nó lento ou fora do ar aparece como pendente: o operador repete a chamada.
            Err(why) => {
                complete = false;
                json!({ "nodeId": node_id, "complete": false, "pending": true, "error": why })
            }
        })
        .collect();
    Ok(Json(json!({ "complete": complete, "nodes": nodes })))
}

async fn cluster_stop(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let backup = state.backup.clone();
    let mut warnings: Vec<String> = Vec::new();
    // Os outros nós param ao mesmo tempo; este, que coordena, para por último e grava a lista.
    let me = state.node.node_id();
    let mut stopped: Vec<String> = Vec::new();
    let mut nodes: Vec<Value> = Vec::new();
    if let Some(cluster) = state.cluster.clone() {
        let others: Vec<_> = cluster
            .membership()
            .table()
            .map(|t| t.members)
            .unwrap_or_default()
            .into_iter()
            .filter(|m| m.node_id != me)
            .collect();
        let mut set = tokio::task::JoinSet::new();
        for member in others {
            let cluster = cluster.clone();
            set.spawn(async move {
                let answer = cluster.call_member(&member, PeerRequest::BackupStop).await;
                (member.node_id, answer)
            });
        }
        while let Some(Ok((node_id, answer))) = set.join_next().await {
            match answer {
                Ok(PeerResponse::Backup(report)) => {
                    if report["drained"].as_bool().unwrap_or(false) {
                        stopped.push(node_id.clone());
                    } else {
                        warnings.push(format!("o nó {node_id} não concluiu o drain no prazo"));
                    }
                    nodes.push(report);
                }
                Ok(_) | Err(_) => {
                    // O nó silencioso fica fora da lista: ao subir será tratado como morto.
                    warnings.push(format!("o nó {node_id} não respondeu à parada"));
                    nodes.push(
                        json!({ "nodeId": node_id, "drained": false, "error": "sem resposta" }),
                    );
                }
            }
        }
    }
    let drained = state.drain.drain_and_wait().await;
    if drained {
        stopped.push(me.clone());
    } else {
        warnings.push(format!("o nó {me} não concluiu o drain no prazo"));
    }
    nodes.push(json!({ "nodeId": me, "drained": drained }));
    let mut stop_list = false;
    match &backup {
        Some(backup) => {
            stop_list = backup.write_stop_list(stopped.clone()).await;
            if !stop_list {
                warnings.push(
                    "o backup não foi enviado ao S3: a lista da parada não foi gravada (o drain em disco foi feito)"
                        .to_owned(),
                );
            }
            // O coordenador é o último a sair: o batimento dele some por último.
            backup.leave().await;
        }
        None => warnings.push("backup em S3 não configurado: não há lista da parada".to_owned()),
    }
    Ok(Json(json!({
        "stopped": stopped,
        "stopList": stop_list,
        "warnings": warnings,
        "nodes": nodes,
    })))
}
