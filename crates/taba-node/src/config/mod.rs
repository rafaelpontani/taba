//! A configuração do nó (doc 10): arquivo `.yml`, variáveis `TABA_*` e padrões.
//!
//! `params` é a tabela de chaves e padrões; `source` junta as três origens e converte
//! os valores; aqui ficam o `Config` tipado e as validações que impedem a partida.
//! A leitura do arquivo e do ambiente é injetada, então os testes não tocam o processo.

mod error;
mod otlp;
mod params;
mod source;

use std::path::{Path, PathBuf};
use std::time::Duration;

use ipnet::IpNet;

pub use error::ConfigError;
pub use otlp::{DEFAULT_METRIC_INTERVAL, Otlp, OtlpProtocol, Sampler};
pub use params::{PARAMS, env_name};
use source::{Bytes, List, NetList, Parse, Source};
pub use source::{EnvSource, MapEnv, ProcessEnv};

/// Uma duração com unidade (`100ms`, `60s`, `5m`, `2h`, `30d`), a mesma forma dos parâmetros.
pub fn parse_duration(text: &str) -> Option<Duration> {
    <Duration as Parse>::parse(text)
}

use crate::core::validation;

/// Caminho que o `.yml` tem se nada for informado. Se não existe, o nó segue só
/// com as variáveis e os padrões.
pub const DEFAULT_CONFIG_FILE: &str = "/etc/taba/taba.yml";

/// Margem do envelope JSON de uma linha dos streams de sincronização, somada ao
/// limite rígido em base64 (doc 10, `cluster.sync.stream.max-line-bytes`).
const LINE_ENVELOPE_MARGIN: u64 = 4096;

/// O token raiz é um segredo de, no mínimo, 32 caracteres.
const ROOT_TOKEN_MIN_LEN: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Production,
    Development,
}

impl Mode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Mode::Production => "production",
            Mode::Development => "development",
        }
    }
}

impl Parse for Mode {
    const EXPECTED: &'static str = "`production` ou `development`";
    fn parse(s: &str) -> Option<Self> {
        match s {
            "production" => Some(Mode::Production),
            "development" => Some(Mode::Development),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthSwitch {
    Auto,
    On,
    Off,
}

impl Parse for AuthSwitch {
    const EXPECTED: &'static str = "`auto`, `true` ou `false`";
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "auto" => Some(AuthSwitch::Auto),
            "true" => Some(AuthSwitch::On),
            "false" => Some(AuthSwitch::Off),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TopicsMetricsMode {
    Off,
    ByTags,
    PerTopic,
}

impl Parse for TopicsMetricsMode {
    const EXPECTED: &'static str = "`off`, `by-tags` ou `per-topic`";
    fn parse(s: &str) -> Option<Self> {
        match s {
            "off" => Some(Self::Off),
            "by-tags" => Some(Self::ByTags),
            "per-topic" => Some(Self::PerTopic),
            _ => None,
        }
    }
}

/// Modo de sincronização do SQLite (`storage.db.synchronous`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbSynchronous {
    Off,
    Normal,
    Full,
    Extra,
}

impl Parse for DbSynchronous {
    const EXPECTED: &'static str = "`OFF`, `NORMAL`, `FULL` ou `EXTRA`";
    fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_uppercase().as_str() {
            "OFF" => Some(Self::Off),
            "NORMAL" => Some(Self::Normal),
            "FULL" => Some(Self::Full),
            "EXTRA" => Some(Self::Extra),
            _ => None,
        }
    }
}

/// Um segredo que nunca aparece em log nem em `Debug`.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Secret(<oculto>)")
    }
}

/// De onde vem o token raiz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootToken {
    None,
    Inline(Secret),
    /// Arquivo com o token atual e, opcionalmente, o anterior (duas linhas).
    File(PathBuf),
}

#[derive(Debug, Clone)]
pub struct Pagination {
    pub default_limit: u32,
    pub max_limit: u32,
}

#[derive(Debug, Clone)]
pub struct Ports {
    pub api: u16,
    pub cluster: u16,
    /// `None`: as rotas de operação ficam na porta da API.
    pub operations: Option<u16>,
}

#[derive(Debug, Clone)]
pub struct Tls {
    /// `None`: a porta do cluster atende em HTTP puro, com aviso no log.
    pub dir: Option<PathBuf>,
    pub reload_interval: Duration,
    pub expiry_warning: Duration,
}

#[derive(Debug, Clone)]
pub struct Auth {
    /// Já resolvido: `auto` virou `true` ou `false`. Sempre `false` em desenvolvimento.
    pub enabled: bool,
    pub root_token: RootToken,
    pub reload_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct Drain {
    pub on_stop: bool,
    pub timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct Storage {
    pub dir: PathBuf,
    pub memory_limit: Option<u64>,
    pub rate_window: Duration,
    pub topic_reserve: u64,
    pub db_synchronous: DbSynchronous,
}

#[derive(Debug, Clone)]
pub struct Messages {
    pub max_bytes: u64,
    pub hard_limit: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Write {
    /// Efetivo: 1 em desenvolvimento, seja qual for o valor configurado.
    pub replication_factor: u32,
    /// Efetivo: 1 em desenvolvimento; senão `floor(replication.factor / 2) + 1` se não definido.
    pub quorum: u32,
    pub retry_attempts: u32,
    pub retry_backoff: Duration,
    pub pending_ttl: Duration,
    pub commit_timeout: Duration,
}

#[derive(Debug, Clone)]
pub struct ClusterSync {
    pub stream_max_items: Option<u64>,
    pub stream_max_bytes: Option<u64>,
    pub stream_max_line_bytes: Option<u64>,
    pub stream_idle_timeout: Option<Duration>,
    pub range_size: Option<u64>,
    pub log_max_bytes: Option<u64>,
    pub timeout: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct Cluster {
    pub entry_address: Option<String>,
    /// `cluster.advertise-addresses`: os IPs que o nó informa aos outros; vazio, ele descobre.
    pub advertise_addresses: Vec<String>,
    pub first_node: bool,
    pub virtual_nodes: u32,
    pub keepalive_interval: Option<Duration>,
    pub keepalive_max_topics: Option<u32>,
    pub notify_window: Duration,
    pub notify_wait: Option<Duration>,
    pub reconnect_retries: Option<u32>,
    pub reconnect_backoff: Option<Duration>,
    pub queue_max_size: Option<u64>,
    pub queue_max_age: Option<Duration>,
    pub vote_quorum_percent: u32,
    pub vote_min_voters: u32,
    pub freezing_duration: Option<Duration>,
    pub bootstrap_retry_interval: Duration,
    /// `0`: sem limite.
    pub bootstrap_retry_max_attempts: u32,
    pub message_loss_ttl: Option<Duration>,
    pub recovery_timeout: Option<Duration>,
    pub reunion_interval: Option<Duration>,
    pub sync: ClusterSync,
}

#[derive(Debug, Clone)]
pub struct Delivery {
    pub visibility_timeout_default: Duration,
    pub visibility_timeout_max: Option<Duration>,
    pub ready_list_size: u32,
    pub invisible_list_size: Option<u32>,
    pub invisible_reload_threshold: Option<u32>,
    pub max_messages_default: u32,
    pub max_messages_max: Option<u32>,
    pub get_max_attempts: Option<u32>,
    pub fair_ready_weight: f64,
    pub fair_max_groups: Option<u32>,
    /// `0` desliga a garantia.
    pub fair_normal_every: u32,
    pub take_max_attempts: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Retention {
    pub ttl_default: Duration,
    pub max_deliveries_default: u32,
    pub sweep_interval: Duration,
}

#[derive(Debug, Clone)]
pub struct Fifo {
    pub dedup_window: Duration,
    pub dedup_index_max_entries: Option<u64>,
    pub forward_max_attempts: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Topics {
    pub dead_letter_suffix: String,
    pub volatile_idle_timeout_default: Duration,
    pub volatile_idle_check_retry_interval: Option<Duration>,
    /// `None`: sem limite.
    pub distributor_max_targets: Option<u32>,
    /// `None`: sem limite.
    pub volatile_max_count: Option<u32>,
    pub volatile_takeover_jitter: Option<Duration>,
}

#[derive(Debug, Clone)]
pub struct Telemetry {
    pub prometheus_enabled: bool,
    pub topics_mode: TopicsMetricsMode,
    pub topics_group_by_tags: Vec<String>,
    pub topics_max_topics: Option<u32>,
    /// `None`: o padrão ainda está a definir; o nó usa `info`.
    pub logs_level: Option<String>,
    /// Atributos de recurso de `OTEL_SERVICE_NAME` e `OTEL_RESOURCE_ATTRIBUTES` (variáveis do
    /// SDK do OpenTelemetry, que não têm chave no YAML).
    pub resource: Vec<(String, String)>,
    /// A exportação OTLP (`OTEL_EXPORTER_OTLP_*`); `None` sem endpoint.
    pub otlp: Option<Otlp>,
}

/// Lê `OTEL_RESOURCE_ATTRIBUTES` (`chave=valor,chave=valor`) e `OTEL_SERVICE_NAME`, que vence
/// o `service.name` da primeira. Par sem `=` ou sem chave é ignorado.
fn resource_attributes(env: &dyn EnvSource) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for pair in env
        .get("OTEL_RESOURCE_ATTRIBUTES")
        .unwrap_or_default()
        .split(',')
    {
        if let Some((key, value)) = pair.split_once('=')
            && !key.trim().is_empty()
        {
            out.retain(|(k, _)| k != key.trim());
            out.push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    if let Some(name) = env
        .get("OTEL_SERVICE_NAME")
        .filter(|n| !n.trim().is_empty())
    {
        out.retain(|(k, _)| k != "service.name");
        out.push(("service.name".to_owned(), name.trim().to_owned()));
    }
    out
}

#[derive(Debug, Clone)]
pub struct Stats {
    pub topics_top_groups: u32,
    pub topics_cache_ttl: Option<Duration>,
    pub topics_backlog_depth: Option<u64>,
    pub topics_backlog_age: Option<Duration>,
    pub topics_noisy_group_share: u32,
    pub topics_group_stalled_deliveries: Option<u32>,
    pub topics_stalled_after: Option<Duration>,
    pub topics_idle_after: Option<Duration>,
    pub disk_free_min: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct RateLimit {
    pub publish_rps: Option<f64>,
    pub consume_rps: Option<f64>,
    pub admin_rps: Option<f64>,
    pub ops_rps: Option<f64>,
    pub burst: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct ClientIp {
    pub headers_enabled: bool,
    pub headers: Vec<String>,
    pub trusted_proxies: Vec<IpNet>,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub mode: Mode,
    pub resume_on_start: bool,
    pub ports: Ports,
    pub pagination: Pagination,
    pub secrets_dir: PathBuf,
    pub tls: Tls,
    pub auth: Auth,
    pub drain: Drain,
    pub storage: Storage,
    pub messages: Messages,
    pub write: Write,
    pub cluster: Cluster,
    pub delivery: Delivery,
    pub retention: Retention,
    pub fifo: Fifo,
    pub topics: Topics,
    pub telemetry: Telemetry,
    pub stats: Stats,
    pub operations_allowed_ips: Vec<IpNet>,
    pub rate_limit: RateLimit,
    pub client_ip: ClientIp,
    /// Avisos da leitura (parâmetros ignorados, variáveis desconhecidas). O nó os
    /// registra no log ao subir.
    pub warnings: Vec<String>,
}

impl Config {
    pub fn is_development(&self) -> bool {
        self.mode == Mode::Development
    }

    /// Lê o arquivo (`--config`, `TABA_CONFIG_FILE` ou o padrão) e o ambiente do processo.
    /// Um caminho informado que não existe impede a subida; o padrão ausente, não.
    pub fn load(cli_path: Option<&Path>) -> Result<Self, ConfigError> {
        let env = ProcessEnv;
        let explicit = cli_path
            .map(Path::to_path_buf)
            .or_else(|| env.get("TABA_CONFIG_FILE").map(PathBuf::from));
        let path = match explicit {
            Some(p) => Some(p),
            None => Some(PathBuf::from(DEFAULT_CONFIG_FILE)).filter(|p| p.is_file()),
        };
        let file = match path {
            Some(p) => {
                let content = std::fs::read_to_string(&p).map_err(|source| ConfigError::Read {
                    path: p.clone(),
                    source,
                })?;
                Some((p.display().to_string(), content))
            }
            None => None,
        };
        Self::parse(
            file.as_ref()
                .map(|(label, content)| (label.as_str(), content.as_str())),
            &env,
        )
    }

    /// Monta a configuração a partir do conteúdo do arquivo e do ambiente.
    pub fn parse(yaml: Option<(&str, &str)>, env: &dyn EnvSource) -> Result<Self, ConfigError> {
        let (source, mut warnings) = Source::build(yaml, env)?;
        let mut config = Self::build(&source, &mut warnings)?;
        config.telemetry.resource = resource_attributes(env);
        config.telemetry.otlp = otlp::from_env(env)?;
        Ok(Self { warnings, ..config })
    }

    fn build(s: &Source, warnings: &mut Vec<String>) -> Result<Self, ConfigError> {
        let mode: Mode = s.get("mode")?;
        let dev = mode == Mode::Development;
        let mut problems: Vec<String> = Vec::new();

        if dev {
            let ignored =
                s.set_keys_with_prefix(&["cluster.", "ring.", "replication.", "write.quorum"]);
            if !ignored.is_empty() {
                warnings.push(format!(
                    "mode=development: parâmetros ignorados num cluster de um só nó: {}",
                    ignored.join(", ")
                ));
            }
        }

        // Portas
        let ports = Ports {
            api: s.get("api.port")?,
            cluster: s.get("cluster.port")?,
            operations: s.opt("operations.port")?,
        };
        check_distinct_ports(&ports, dev, &mut problems);
        let pagination = Pagination {
            default_limit: s.get("api.pagination.default-limit")?,
            max_limit: s.get("api.pagination.max-limit")?,
        };
        if pagination.default_limit == 0 || pagination.default_limit > pagination.max_limit {
            problems.push(
                "api.pagination.default-limit deve estar entre 1 e api.pagination.max-limit".into(),
            );
        }

        // Segurança: pasta de segredos, TLS do cluster e token raiz
        let secrets_dir: PathBuf = PathBuf::from(s.get::<String>("secrets.dir")?);
        let tls = Tls {
            dir: if dev {
                None
            } else {
                resolve_tls_dir(s, &secrets_dir, &mut problems)?
            },
            reload_interval: s.get("cluster.tls.reload-interval")?,
            expiry_warning: s.get("cluster.tls.expiry-warning")?,
        };
        let auth = resolve_auth(s, &secrets_dir, dev, &mut problems)?;

        let drain = Drain {
            on_stop: s.get("drain.on-stop")?,
            timeout: s.get("drain.timeout")?,
        };

        // Armazenamento e mensagens
        let storage = Storage {
            dir: PathBuf::from(s.get::<String>("storage.dir")?),
            memory_limit: s.opt::<Bytes>("memory.limit")?.map(|b| b.0),
            rate_window: s.get("memory.rate.window")?,
            topic_reserve: s.get("memory.topic.reserve")?,
            db_synchronous: s.get("storage.db.synchronous")?,
        };
        if storage.dir.file_name().is_none_or(|n| n != "taba") {
            problems.push(format!(
                "storage.dir (`{}`): a última pasta deve se chamar `taba`",
                storage.dir.display()
            ));
        }
        let messages = Messages {
            max_bytes: s.get::<Bytes>("message.max-bytes")?.0,
            hard_limit: s.opt::<Bytes>("message.hard-limit")?.map(|b| b.0),
        };
        if messages.max_bytes == 0 {
            problems.push("message.max-bytes deve ser maior que zero".into());
        }
        if let Some(hard) = messages.hard_limit
            && messages.max_bytes > hard
        {
            problems.push(format!(
                "message.max-bytes ({}) não pode passar de message.hard-limit ({hard})",
                messages.max_bytes
            ));
        }

        // Escrita e durabilidade
        let configured_factor: u32 = s.get("replication.factor")?;
        let configured_quorum: Option<u32> = s.opt("write.quorum")?;
        if configured_factor == 0 {
            problems.push("replication.factor deve ser de 1 em diante".into());
        }
        let default_quorum = configured_factor / 2 + 1;
        let write = if dev {
            Write {
                replication_factor: 1,
                quorum: 1,
                ..write_rest(s)?
            }
        } else {
            let quorum = configured_quorum.unwrap_or(default_quorum);
            if configured_factor > 0 && !(1..=configured_factor).contains(&quorum) {
                problems.push(format!(
                    "write.quorum ({quorum}) deve estar entre 1 e replication.factor ({configured_factor})"
                ));
            }
            Write {
                replication_factor: configured_factor,
                quorum,
                ..write_rest(s)?
            }
        };

        // Cluster
        let cluster = cluster_config(s)?;
        match (&cluster.entry_address, dev) {
            (None, false) => {
                problems.push("cluster.entry-address é obrigatório em mode=production".into());
            }
            (Some(address), false) if !is_host_port(address) => problems.push(format!(
                "cluster.entry-address (`{address}`) deve ser host:porta, na porta do cluster"
            )),
            _ => {}
        }
        if !(1..=100).contains(&cluster.vote_quorum_percent) {
            problems.push("cluster.vote.quorum-percent deve estar entre 1 e 100".into());
        }
        if cluster.vote_min_voters == 0 {
            problems.push("cluster.vote.min-voters deve ser de 1 em diante".into());
        }
        if cluster.virtual_nodes == 0 {
            problems.push("ring.virtual-nodes deve ser de 1 em diante".into());
        }
        if write.retry_attempts == 0 {
            problems.push("write.retry.attempts deve ser de 1 em diante".into());
        }
        if let (Some(hard), Some(line)) = (messages.hard_limit, cluster.sync.stream_max_line_bytes)
        {
            let needed = hard.div_ceil(3) * 4 + LINE_ENVELOPE_MARGIN;
            if line < needed {
                problems.push(format!(
                    "cluster.sync.stream.max-line-bytes ({line}) precisa caber message.hard-limit em base64 \
                     mais o envelope JSON (pelo menos {needed})"
                ));
            }
        }

        // Entrega, retenção, FIFO, tópicos
        let delivery = delivery_config(s)?;
        if let Some(max) = delivery.visibility_timeout_max
            && delivery.visibility_timeout_default > max
        {
            problems.push("delivery.visibility-timeout.default não pode passar do máximo".into());
        }
        if let Some(max) = delivery.max_messages_max
            && delivery.max_messages_default > max
        {
            problems.push("delivery.max-messages.default não pode passar do máximo".into());
        }
        if !(0.0..=1.0).contains(&delivery.fair_ready_weight) {
            problems.push("delivery.fair.ready-weight deve estar entre 0 e 1".into());
        }

        let retention = Retention {
            ttl_default: s.get("retention.ttl.default")?,
            max_deliveries_default: s.get("retention.max-deliveries.default")?,
            sweep_interval: s.get("retention.sweep.interval")?,
        };
        if let Err(e) = validation::in_range(
            "retention.ttl.default",
            retention.ttl_default.as_secs(),
            validation::TTL_SECS,
        ) {
            problems.push(e.message);
        }
        if let Err(e) = validation::in_range(
            "retention.max-deliveries.default",
            u64::from(retention.max_deliveries_default),
            validation::MAX_DELIVERIES,
        ) {
            problems.push(e.message);
        }

        let fifo = Fifo {
            dedup_window: s.get("fifo.dedup.window")?,
            dedup_index_max_entries: s.opt("fifo.dedup.index.max-entries")?,
            forward_max_attempts: s.opt("fifo.forward.max-attempts")?,
        };

        let topics = Topics {
            dead_letter_suffix: s.get("topics.dead-letter.suffix")?,
            volatile_idle_timeout_default: s.get("topics.volatile.idle-timeout.default")?,
            volatile_idle_check_retry_interval: s
                .opt("topics.volatile.idle-check.retry-interval")?,
            distributor_max_targets: s.opt("topics.distributor.max-targets")?,
            volatile_max_count: s.opt("topics.volatile.max-count")?,
            volatile_takeover_jitter: s.opt("topics.volatile.takeover.jitter")?,
        };
        let suffix = &topics.dead_letter_suffix;
        if suffix.is_empty()
            || !suffix
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        {
            problems.push(
                "topics.dead-letter.suffix deve ter ao menos um caractere de A-Za-z0-9_-".into(),
            );
        }
        if let Err(e) = validation::in_range(
            "topics.volatile.idle-timeout.default",
            topics.volatile_idle_timeout_default.as_secs(),
            validation::IDLE_TIMEOUT_SECS,
        ) {
            problems.push(e.message);
        }

        // Observabilidade e rate limits
        let telemetry = Telemetry {
            prometheus_enabled: s.get("telemetry.prometheus.enabled")?,
            topics_mode: s.get("telemetry.topics.mode")?,
            topics_group_by_tags: s
                .opt::<List>("telemetry.topics.group-by-tags")?
                .map(|l| l.0)
                .unwrap_or_default(),
            topics_max_topics: s.opt("telemetry.topics.max-topics")?,
            logs_level: s.opt("telemetry.logs.level")?,
            resource: Vec::new(),
            otlp: None,
        };
        let stats = Stats {
            topics_top_groups: s.get("stats.topics.top-groups")?,
            topics_cache_ttl: s.opt("stats.topics.cache-ttl")?,
            topics_backlog_depth: s.opt("stats.topics.backlog.depth")?,
            topics_backlog_age: s.opt("stats.topics.backlog.age")?,
            topics_noisy_group_share: s.get("stats.topics.noisy-group-share")?,
            topics_group_stalled_deliveries: s.opt("stats.topics.group-stalled-deliveries")?,
            topics_stalled_after: s.opt("stats.topics.stalled-after")?,
            topics_idle_after: s.opt("stats.topics.idle-after")?,
            disk_free_min: s.opt::<Bytes>("stats.disk.free-min")?.map(|b| b.0),
        };
        let operations_allowed_ips = s
            .opt::<NetList>("operations.allowed-ips")?
            .map(|l| l.0)
            .unwrap_or_default();
        let rate_limit = RateLimit {
            publish_rps: s.opt("ratelimit.publish.rps")?,
            consume_rps: s.opt("ratelimit.consume.rps")?,
            admin_rps: s.opt("ratelimit.admin.rps")?,
            ops_rps: s.opt("ratelimit.ops.rps")?,
            burst: s.opt("ratelimit.burst")?,
        };
        let client_ip = ClientIp {
            headers_enabled: s.get("client-ip.headers.enabled")?,
            headers: s.get::<List>("client-ip.headers")?.0,
            trusted_proxies: s
                .opt::<NetList>("client-ip.trusted-proxies")?
                .map(|l| l.0)
                .unwrap_or_default(),
        };

        if let Some(level) = &telemetry.logs_level
            && !["trace", "debug", "info", "warn", "error"].contains(&level.as_str())
        {
            problems.push(format!(
                "telemetry.logs.level (`{level}`) deve ser trace, debug, info, warn ou error"
            ));
        }

        if !problems.is_empty() {
            return Err(ConfigError::Validation(problems));
        }

        Ok(Config {
            mode,
            resume_on_start: s.get("development.resume-on-start")?,
            ports,
            pagination,
            secrets_dir,
            tls,
            auth,
            drain,
            storage,
            messages,
            write,
            cluster,
            delivery,
            retention,
            fifo,
            topics,
            telemetry,
            stats,
            operations_allowed_ips,
            rate_limit,
            client_ip,
            warnings: Vec::new(),
        })
    }
}

/// As três portas precisam ser diferentes entre si; `0` (qualquer porta livre, só
/// nos testes) nunca conta como repetida. Em desenvolvimento a do cluster não é aberta.
fn check_distinct_ports(ports: &Ports, dev: bool, problems: &mut Vec<String>) {
    let mut named = vec![("api.port", ports.api)];
    if !dev {
        named.push(("cluster.port", ports.cluster));
    }
    if let Some(p) = ports.operations {
        named.push(("operations.port", p));
    }
    for (i, (a_name, a)) in named.iter().enumerate() {
        for (b_name, b) in &named[i + 1..] {
            if a == b && *a != 0 {
                problems.push(format!("{a_name} e {b_name} usam a mesma porta ({a}); as três portas precisam ser diferentes"));
            }
        }
    }
}

/// `host:porta`, com a porta de 1 a 65535 (o formato de `cluster.entry-address`).
fn is_host_port(address: &str) -> bool {
    address
        .trim()
        .rsplit_once(':')
        .is_some_and(|(host, port)| !host.is_empty() && port.parse::<u16>().is_ok_and(|p| p != 0))
}

/// `cluster.tls.dir`: o definido precisa existir; o padrão (`<secrets.dir>/tls`) só vale
/// se existir. Com pasta, é preciso ao menos um par `<nome>.crt` + `<nome>.key`.
fn resolve_tls_dir(
    s: &Source,
    secrets_dir: &Path,
    problems: &mut Vec<String>,
) -> Result<Option<PathBuf>, ConfigError> {
    let explicit: Option<String> = s.opt("cluster.tls.dir")?;
    let dir = match explicit {
        Some(d) => {
            let d = PathBuf::from(d);
            if !d.is_dir() {
                problems.push(format!("cluster.tls.dir (`{}`) não existe", d.display()));
                return Ok(None);
            }
            d
        }
        None => {
            let d = secrets_dir.join("tls");
            if !d.is_dir() {
                return Ok(None);
            }
            d
        }
    };
    if !has_cert_pair(&dir) {
        problems.push(format!(
            "cluster.tls.dir (`{}`) não tem nenhum par <nome>.crt + <nome>.key",
            dir.display()
        ));
    }
    Ok(Some(dir))
}

fn has_cert_pair(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .map(|e| e.path())
        .any(|p| p.extension().is_some_and(|e| e == "crt") && p.with_extension("key").is_file())
}

/// Token raiz e a chave `auth.enabled`. O arquivo ganha do valor inline; o definido
/// precisa existir, e o padrão (`<secrets.dir>/root-token`) só vale se existir.
fn resolve_auth(
    s: &Source,
    secrets_dir: &Path,
    dev: bool,
    problems: &mut Vec<String>,
) -> Result<Auth, ConfigError> {
    let reload_interval = s.get("auth.reload-interval")?;
    let switch: AuthSwitch = s.get("auth.enabled")?;
    let inline: Option<String> = s.opt("auth.root-token")?;
    let file: Option<String> = s.opt("auth.root-token-file")?;
    if dev {
        return Ok(Auth {
            enabled: false,
            root_token: RootToken::None,
            reload_interval,
        });
    }

    let file_path = match file {
        Some(f) => {
            let f = PathBuf::from(f);
            if !f.is_file() {
                problems.push(format!(
                    "auth.root-token-file (`{}`) não existe",
                    f.display()
                ));
                None
            } else {
                Some(f)
            }
        }
        None => Some(secrets_dir.join("root-token")).filter(|p| p.is_file()),
    };

    let root_token = if let Some(path) = file_path {
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let lines: Vec<&str> = text
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .collect();
                if lines.is_empty()
                    || lines.len() > 2
                    || lines.iter().any(|l| l.chars().count() < ROOT_TOKEN_MIN_LEN)
                {
                    problems.push(format!(
                        "auth.root-token-file (`{}`): use uma ou duas linhas (a atual e a anterior), de no mínimo {ROOT_TOKEN_MIN_LEN} caracteres",
                        path.display()
                    ));
                }
            }
            Err(e) => problems.push(format!("auth.root-token-file (`{}`): {e}", path.display())),
        }
        RootToken::File(path)
    } else if let Some(token) = inline {
        if token.chars().count() < ROOT_TOKEN_MIN_LEN {
            problems.push(format!(
                "auth.root-token deve ter no mínimo {ROOT_TOKEN_MIN_LEN} caracteres"
            ));
        }
        RootToken::Inline(Secret(token))
    } else {
        RootToken::None
    };

    let has_token = root_token != RootToken::None;
    let enabled = match switch {
        AuthSwitch::Auto => has_token,
        AuthSwitch::Off => false,
        AuthSwitch::On => {
            if !has_token {
                problems.push("auth.enabled=true exige um token raiz (auth.root-token, auth.root-token-file ou o root-token da pasta de segredos)".into());
            }
            true
        }
    };
    Ok(Auth {
        enabled,
        root_token,
        reload_interval,
    })
}

fn write_rest(s: &Source) -> Result<Write, ConfigError> {
    Ok(Write {
        replication_factor: 0,
        quorum: 0,
        retry_attempts: s.get("write.retry.attempts")?,
        retry_backoff: s.get("write.retry.backoff")?,
        pending_ttl: s.get("write.pending.ttl")?,
        commit_timeout: s.get("write.commit.timeout")?,
    })
}

fn cluster_config(s: &Source) -> Result<Cluster, ConfigError> {
    Ok(Cluster {
        entry_address: s.opt("cluster.entry-address")?,
        advertise_addresses: s
            .opt::<List>("cluster.advertise-addresses")?
            .map(|l| l.0)
            .unwrap_or_default(),
        first_node: s.get("cluster.first-node")?,
        virtual_nodes: s.get("ring.virtual-nodes")?,
        keepalive_interval: s.opt("cluster.channel.keepalive.interval")?,
        keepalive_max_topics: s.opt("cluster.channel.keepalive.max-topics")?,
        notify_window: s.get("cluster.notify.window")?,
        notify_wait: s.opt("cluster.notify.wait")?,
        reconnect_retries: s.opt("cluster.channel.reconnect.retries")?,
        reconnect_backoff: s.opt("cluster.channel.reconnect.backoff")?,
        queue_max_size: s.opt("cluster.channel.queue.max-size")?,
        queue_max_age: s.opt("cluster.channel.queue.max-age")?,
        vote_quorum_percent: s.get("cluster.vote.quorum-percent")?,
        vote_min_voters: s.get("cluster.vote.min-voters")?,
        freezing_duration: s.opt("cluster.freezing.duration")?,
        bootstrap_retry_interval: s.get("cluster.bootstrap.retry.interval")?,
        bootstrap_retry_max_attempts: s.get("cluster.bootstrap.retry.max-attempts")?,
        message_loss_ttl: s.opt("cluster.message-loss.ttl")?,
        recovery_timeout: s.opt("cluster.recovery.timeout")?,
        reunion_interval: s.opt("cluster.reunion.interval")?,
        sync: ClusterSync {
            stream_max_items: s.opt("cluster.sync.stream.max-items")?,
            stream_max_bytes: s
                .opt::<Bytes>("cluster.sync.stream.max-bytes")?
                .map(|b| b.0),
            stream_max_line_bytes: s
                .opt::<Bytes>("cluster.sync.stream.max-line-bytes")?
                .map(|b| b.0),
            stream_idle_timeout: s.opt("cluster.sync.stream.idle-timeout")?,
            range_size: s.opt("cluster.sync.range.size")?,
            log_max_bytes: s.opt::<Bytes>("cluster.sync.log.max-bytes")?.map(|b| b.0),
            timeout: s.opt("cluster.sync.timeout")?,
        },
    })
}

fn delivery_config(s: &Source) -> Result<Delivery, ConfigError> {
    Ok(Delivery {
        visibility_timeout_default: s.get("delivery.visibility-timeout.default")?,
        visibility_timeout_max: s.opt("delivery.visibility-timeout.max")?,
        ready_list_size: s.get("delivery.ready-list.size")?,
        invisible_list_size: s.opt("delivery.invisible-list.size")?,
        invisible_reload_threshold: s.opt("delivery.invisible.reload-threshold")?,
        max_messages_default: s.get("delivery.max-messages.default")?,
        max_messages_max: s.opt("delivery.max-messages.max")?,
        get_max_attempts: s.opt("delivery.get.max-attempts")?,
        fair_ready_weight: s.get("delivery.fair.ready-weight")?,
        fair_max_groups: s.opt("delivery.fair.max-groups")?,
        fair_normal_every: s.get("delivery.fair.normal-every")?,
        take_max_attempts: s.opt("delivery.take.max-attempts")?,
    })
}

#[cfg(test)]
mod tests;
