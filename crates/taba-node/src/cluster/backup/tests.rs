use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use super::heartbeat::Verdict;
use super::*;
use crate::cluster::state::NodeShared;
use crate::config::{Config, DbSynchronous, MapEnv, Mode};
use crate::core::clock::SystemClock;
use crate::engine::messages::{MessageRules, Messages};
use crate::engine::tokens::{TokenRules, Tokens};
use crate::engine::topics::{TopicRules, Topics};
use crate::storage::backup::{BackupDb, MemoryStore, ObjectStore, keys};
use crate::storage::tokens::MemoryTokenStore;
use crate::storage::topics::MemoryTopicStore;

/// Um nó só (sem cluster) com o backup ligado a um depósito em memória. Os prazos de tempo real
/// (batimento e morte) são curtos.
struct Fixture {
    backup: Arc<Backup>,
    s3: Arc<MemoryStore>,
    _dir: tempfile::TempDir,
}

fn fixture_on(s3: Arc<MemoryStore>, node_id: &str) -> Fixture {
    let config = Config::parse(
        None,
        &MapEnv::new([
            ("TABA_MODE", "development"),
            ("TABA_BACKUP_S3_BUCKET", "teste"),
            ("TABA_BACKUP_HEARTBEAT_INTERVAL", "50ms"),
            ("TABA_BACKUP_DEAD_AFTER", "400ms"),
            ("TABA_BACKUP_RESTORE_LOCK_TTL", "300ms"),
            ("TABA_BACKUP_DRAIN_TIMEOUT", "2s"),
        ]),
    )
    .unwrap();
    let clock = Arc::new(SystemClock);
    let topics = Arc::new(Topics::new(
        TopicRules::from_config(&config),
        Arc::new(MemoryTopicStore::default()),
        clock.clone(),
        Vec::new(),
    ));
    let dir = tempfile::tempdir().unwrap();
    let disk = Arc::new(MessageStore::new(dir.path(), DbSynchronous::Normal));
    let messages = Messages::new(
        topics.clone(),
        clock.clone(),
        MessageRules::from_config(&config),
        disk.clone(),
        false,
    );
    let tokens = Arc::new(Tokens::new(
        TokenRules::from_config(&config),
        Arc::new(MemoryTokenStore::default()),
        clock.clone(),
        Vec::new(),
    ));
    let shared = Arc::new(NodeShared::new(
        node_id.into(),
        Mode::Development,
        std::time::SystemTime::now(),
    ));
    let backup = Backup::new(BackupArgs {
        rules: BackupRules::from_config(&config),
        store: s3.clone(),
        db: Arc::new(BackupDb::open(dir.path()).unwrap()),
        messages,
        disk,
        topics,
        tokens,
        clock,
        shared: shared.clone(),
        topology: Arc::new(Solo::new(shared)),
        cancel: CancellationToken::new(),
    });
    Fixture {
        backup,
        s3,
        _dir: dir,
    }
}

fn fixture() -> Fixture {
    fixture_on(Arc::new(MemoryStore::new()), "no-a")
}

fn beat_json(id: &str, seq: u64, state: &str) -> Vec<u8> {
    serde_json::to_vec(&Heartbeat {
        id: id.into(),
        addresses: vec!["10.0.0.9".into()],
        port: 8081,
        seq,
        state: state.into(),
    })
    .unwrap()
}

async fn put_beat(s3: &MemoryStore, id: &str, seq: u64, state: &str) {
    s3.put(&keys::node(id), beat_json(id, seq, state))
        .await
        .unwrap();
}

/// UC: UC-BKP-020
#[tokio::test]
async fn a_heartbeat_is_fresh_while_its_etag_changes_and_dead_when_it_stops() {
    let f = fixture();
    put_beat(&f.s3, "outro", 1, "member").await;
    f.backup.observe().await;
    assert_eq!(
        f.backup.liveness("outro"),
        Liveness::Fresh,
        "sem histórico: fresco"
    );

    // Bate de novo: o ETag muda e o prazo recomeça.
    tokio::time::sleep(Duration::from_millis(250)).await;
    put_beat(&f.s3, "outro", 2, "member").await;
    f.backup.observe().await;
    tokio::time::sleep(Duration::from_millis(250)).await;
    f.backup.observe().await;
    assert_eq!(f.backup.liveness("outro"), Liveness::Fresh);

    // Pára de bater: vence por `backup.dead-after`, sem comparar horário nenhum.
    tokio::time::sleep(Duration::from_millis(450)).await;
    f.backup.observe().await;
    assert_eq!(f.backup.liveness("outro"), Liveness::Dead);
}

/// UC: UC-BKP-020
#[tokio::test]
async fn a_node_without_a_file_is_dead_and_an_unreadable_bucket_says_nothing() {
    let f = fixture();
    f.backup.observe().await;
    assert_eq!(
        f.backup.liveness("quem"),
        Liveness::Dead,
        "saiu limpo ou nunca bateu"
    );
    f.s3.set_down(true);
    f.backup.observe().await;
    assert_eq!(
        f.backup.liveness("quem"),
        Liveness::Unknown,
        "o S3 fora do ar não diz nada: vale o comportamento sem o backup"
    );
}

/// UC: UC-BKP-019
#[tokio::test]
async fn the_node_writes_its_own_heartbeat_with_a_growing_sequence() {
    let f = fixture();
    f.backup.heartbeat_pass().await;
    let first: Heartbeat =
        serde_json::from_slice(&f.s3.peek(&keys::node("no-a")).unwrap()).unwrap();
    f.backup.heartbeat_pass().await;
    let second: Heartbeat =
        serde_json::from_slice(&f.s3.peek(&keys::node("no-a")).unwrap()).unwrap();
    assert_eq!(first.id, "no-a");
    assert_eq!(second.seq, first.seq + 1, "sem o seq o ETag não mudaria");
    assert_eq!(second.state, "member");
    f.backup.leave().await;
    assert!(
        f.s3.peek(&keys::node("no-a")).is_none(),
        "sair limpo remove o arquivo"
    );
    f.backup.heartbeat_pass().await;
    assert!(
        f.s3.peek(&keys::node("no-a")).is_none(),
        "depois de sair o nó não bate mais"
    );
}

/// UC: UC-BKP-023, UC-BKP-024
#[tokio::test]
async fn the_verdict_tells_a_live_cluster_from_a_dead_one_from_an_empty_bucket() {
    // Bucket vazio.
    let f = fixture();
    assert_eq!(f.backup.wait_for_verdict().await, Verdict::Empty);

    // Um nó que só está subindo não prova que o cluster existe.
    put_beat(&f.s3, "subindo", 1, "joining").await;
    assert_eq!(f.backup.wait_for_verdict().await, Verdict::Empty);

    // Um nó que bate: o cluster está vivo e o nó entra pelo caminho normal.
    put_beat(&f.s3, "vivo", 1, "member").await;
    let beater = {
        let s3 = f.s3.clone();
        tokio::spawn(async move {
            for seq in 2..=u64::MAX {
                put_beat(&s3, "vivo", seq, "member").await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    };
    match f.backup.wait_for_verdict().await {
        Verdict::Alive(beats) => {
            assert_eq!(beats[0].id, "vivo");
            assert_eq!(beats[0].addresses, ["10.0.0.9"], "os ips para entrar");
        }
        other => panic!("esperava um cluster vivo, veio {other:?}"),
    }
    beater.abort();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Parou de bater: todos vencidos, o nó é o sobrevivente único.
    f.s3.delete(&keys::node("subindo")).await.unwrap();
    assert_eq!(f.backup.wait_for_verdict().await, Verdict::AllDead);

    // O S3 fora do ar.
    f.s3.set_down(true);
    assert_eq!(f.backup.wait_for_verdict().await, Verdict::Unavailable);
}

/// UC: UC-BKP-024, UC-BKP-025
#[tokio::test]
async fn only_one_survivor_gets_the_restore_lock_and_the_other_waits_as_a_normal_node() {
    let s3 = Arc::new(MemoryStore::new());
    put_beat(&s3, "morto", 1, "member").await;
    let a = fixture_on(s3.clone(), "no-a");
    let b = fixture_on(s3.clone(), "no-b");
    // Os dois veem o único arquivo parado (vencido) e disputam a trava.
    let (da, db) = tokio::join!(
        a.backup.startup_decision(false),
        b.backup.startup_decision(false)
    );
    let winners = [&da, &db]
        .iter()
        .filter(|d| ***d == StartupDecision::StartAlone)
        .count();
    assert_eq!(winners, 1, "uma trava, um restaurador: {da:?} / {db:?}");
    assert!(s3.peek(keys::RESTORE_LOCK).is_some());

    // O titular some sem renovar: a trava vence por `backup.restore.lock-ttl` e o outro a toma.
    let loser = if da == StartupDecision::StartAlone {
        &b
    } else {
        &a
    };
    let mut decision = StartupDecision::KeepTrying;
    for _ in 0..20 {
        decision = loser.backup.startup_decision(false).await;
        if decision == StartupDecision::StartAlone {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        decision,
        StartupDecision::StartAlone,
        "a trava vencida foi tomada"
    );
}

/// UC: UC-BKP-025
#[tokio::test]
async fn without_conditional_writes_the_lock_is_unsafe_but_the_node_still_starts() {
    let s3 = Arc::new(MemoryStore::new());
    s3.set_conditional(false);
    put_beat(&s3, "morto", 1, "member").await;
    let a = fixture_on(s3.clone(), "no-a");
    assert_eq!(
        a.backup.startup_decision(false).await,
        StartupDecision::StartAlone,
        "risco aceito (documentado): sem escrita condicional a trava não é segura"
    );
}

/// UC: UC-BKP-023, UC-BKP-024
#[tokio::test]
async fn a_node_with_no_cluster_in_the_bucket_only_starts_alone_if_it_is_the_first_or_the_stop_list_says_so()
 {
    let f = fixture();
    assert_eq!(
        f.backup.startup_decision(false).await,
        StartupDecision::KeepTrying,
        "sem o `first-node` e sem rastro de cluster, espera um cluster aparecer"
    );
    let first = fixture();
    assert_eq!(
        first.backup.startup_decision(true).await,
        StartupDecision::StartAlone
    );

    // Parada programada: a lista diz que o cluster existia e parou de propósito.
    let stopped = fixture();
    assert!(
        stopped
            .backup
            .write_stop_list(vec!["a".into(), "b".into()])
            .await
    );
    let list = stopped.backup.read_stop_list().await.unwrap();
    assert_eq!(list.nodes, ["a", "b"], "na ordem em que pararam");
    assert_eq!(
        stopped.backup.startup_decision(false).await,
        StartupDecision::StartAlone
    );

    // O S3 fora do ar: o recurso fica desligado e vale o comportamento de sempre.
    let down = fixture();
    down.s3.set_down(true);
    assert_eq!(
        down.backup.startup_decision(false).await,
        StartupDecision::KeepTrying
    );
    assert_eq!(
        down.backup.startup_decision(true).await,
        StartupDecision::StartAlone
    );
}

/// UC: UC-BKP-027
#[tokio::test]
async fn cleaning_nodes_removes_only_dead_files_of_nodes_that_are_not_in_the_cluster() {
    let f = fixture();
    put_beat(&f.s3, "fantasma", 1, "member").await;
    put_beat(&f.s3, "outra-particao", 1, "member").await;
    let beater = {
        let s3 = f.s3.clone();
        tokio::spawn(async move {
            for seq in 2..=u64::MAX {
                put_beat(&s3, "outra-particao", seq, "member").await;
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        })
    };
    // Vê os arquivos por mais que `dead-after`: o do fantasma não muda.
    for _ in 0..12 {
        f.backup.observe().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    f.backup.heartbeat_pass().await;
    f.backup.cleanup_nodes().await;
    assert!(
        f.s3.peek(&keys::node("fantasma")).is_none(),
        "morto e fora do cluster"
    );
    assert!(
        f.s3.peek(&keys::node("outra-particao")).is_some(),
        "um arquivo fresco nunca é apagado: pode ser um nó vivo em outra partição"
    );
    assert!(
        f.s3.peek(&keys::node("no-a")).is_some(),
        "o do próprio nó, membro"
    );
    beater.abort();
}

/// UC: UC-BKP-002
#[test]
fn the_backup_rules_come_from_the_configuration_with_the_documented_defaults() {
    let config = Config::parse(None, &MapEnv::new([("TABA_MODE", "development")])).unwrap();
    assert!(
        !config.backup.enabled(),
        "sem bucket o recurso fica desligado"
    );
    let rules = BackupRules::from_config(&config);
    assert_eq!(rules.delay, Duration::from_secs(60));
    assert_eq!(rules.meta_interval, Duration::from_secs(60));
    assert_eq!(rules.segment_max_bytes, 8 * 1024 * 1024);
    assert_eq!(rules.segment_max_messages, 5000);
    assert_eq!(rules.segment_max_age, Duration::from_secs(60));
    assert_eq!(rules.heartbeat_interval, Duration::from_secs(15));
    assert_eq!(rules.dead_after, Duration::from_secs(60));
    assert_eq!(rules.drain_timeout, Duration::from_secs(30));
    assert_eq!(
        (rules.evict_above, rules.evict_to, rules.pause_at),
        (85, 70, 90)
    );
}

/// UC: UC-BKP-002
#[test]
fn the_s3_settings_follow_the_endpoint_and_the_credentials_are_optional() {
    let aws = Config::parse(
        None,
        &MapEnv::new([
            ("TABA_MODE", "development"),
            ("TABA_BACKUP_S3_BUCKET", "meu-bucket"),
            ("TABA_BACKUP_S3_REGION", "sa-east-1"),
        ]),
    )
    .unwrap();
    let s3 = aws.backup.s3.unwrap();
    assert_eq!(s3.bucket, "meu-bucket");
    assert_eq!(s3.region.as_deref(), Some("sa-east-1"));
    assert_eq!(s3.prefix, "taba");
    assert!(
        s3.access_key.is_none(),
        "sem chaves: a cadeia padrão da AWS"
    );
    assert!(!s3.path_style);

    let minio = Config::parse(
        None,
        &MapEnv::new([
            ("TABA_MODE", "development"),
            ("TABA_BACKUP_S3_BUCKET", "taba"),
            ("TABA_BACKUP_S3_ENDPOINT", "http://minio:9000"),
            ("TABA_BACKUP_S3_ACCESS_KEY", "chave"),
            ("TABA_BACKUP_S3_SECRET_KEY", "segredo"),
        ]),
    )
    .unwrap();
    let s3 = minio.backup.s3.unwrap();
    assert!(s3.path_style, "fora da AWS o bucket vai no caminho");
    assert_eq!(s3.secret_key.as_ref().unwrap().expose(), "segredo");
    assert!(
        !format!("{s3:?}").contains("segredo"),
        "o segredo nunca aparece em log nem em Debug"
    );

    // Uma chave sem a outra, ou limites que se contradizem, impedem a partida.
    for bad in [
        vec![
            ("TABA_BACKUP_S3_BUCKET", "b"),
            ("TABA_BACKUP_S3_ACCESS_KEY", "so-uma"),
        ],
        vec![("TABA_BACKUP_DISK_EVICT_TO", "90")],
        vec![("TABA_BACKUP_DISK_PAUSE_AT", "101")],
        vec![
            ("TABA_BACKUP_DEAD_AFTER", "10s"),
            ("TABA_BACKUP_HEARTBEAT_INTERVAL", "15s"),
        ],
        vec![("TABA_BACKUP_SEGMENT_MAX_MESSAGES", "0")],
    ] {
        let mut map = MapEnv::new([("TABA_MODE", "development")]);
        for (key, value) in &bad {
            map.0.insert((*key).to_owned(), (*value).to_owned());
        }
        assert!(
            Config::parse(None, &map).is_err(),
            "{bad:?} deveria ser recusado"
        );
    }
}

/// UC: UC-BKP-002
#[test]
fn the_example_configuration_files_are_valid() {
    for (name, text) in [
        (
            "taba-minio.yml",
            include_str!("../../../../../exemplos/backup-s3/taba-minio.yml"),
        ),
        (
            "taba-aws.yml",
            include_str!("../../../../../exemplos/backup-s3/taba-aws.yml"),
        ),
    ] {
        let config = Config::parse(
            Some((name, text)),
            &MapEnv::new([("TABA_MODE", "development")]),
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));
        let s3 = config
            .backup
            .s3
            .as_ref()
            .unwrap_or_else(|| panic!("{name}: sem bucket"));
        assert!(!s3.bucket.is_empty(), "{name}");
    }
    let minio = Config::parse(
        Some((
            "taba-minio.yml",
            include_str!("../../../../../exemplos/backup-s3/taba-minio.yml"),
        )),
        &MapEnv::new([("TABA_MODE", "development")]),
    )
    .unwrap();
    let s3 = minio.backup.s3.unwrap();
    assert_eq!(s3.endpoint.as_deref(), Some("http://minio:9000"));
    assert!(s3.path_style);
    assert_eq!(minio.backup.dead_after, Duration::from_secs(60));
    assert_eq!(minio.backup.drain_timeout, Duration::from_secs(30));
    assert_eq!(minio.backup.evict_above, 85);
}
