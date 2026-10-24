//! The OpenAPI contract of the two published APIs (RF-22, doc 15): the **client API** (`api.port`) and the
//! **monitoring API** (`operations.port`). The cluster API (mutual TLS, only the other nodes) is internal and is not
//! published.
//!
//! The documents are built from the routes themselves (`utoipa-axum`: each handler is annotated with its path and
//! registered together with it), so a route and its documentation cannot drift apart; `tests/openapi.rs` also checks
//! real responses against the schemas. Each node serves its document on its own port, open (no token), with an
//! interface at `/docs` that is embedded in the binary (nothing is fetched from the internet):
//!
//! | API | Document | Interface |
//! |---|---|---|
//! | client (`api.port`) | `GET /openapi/client.json` | `/docs` |
//! | monitoring (`operations.port`) | `GET /openapi/monitoring.json` | `/docs` |
//!
//! `taba openapi client|monitoring` prints the same documents without starting a node.

mod schemas;

pub use schemas::*;

use axum::Router;
use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};
use utoipa::{Modify, OpenApi};
use utoipa_swagger_ui::SwaggerUi;

use super::AppState;

/// The name of the bearer security scheme.
pub const BEARER: &str = "bearer";

/// Sets what every document shares: the version (the node's) and the bearer scheme.
struct Common;

impl Modify for Common {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        openapi.info.version = env!("CARGO_PKG_VERSION").to_owned();
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            BEARER,
            SecurityScheme::Http(
                HttpBuilder::new()
                    .scheme(HttpAuthScheme::Bearer)
                    .description(Some(
                        "`Authorization: Bearer <token>`. A node that has neither a root token nor `auth.dev-token` \
                         runs with authentication off and takes no token.",
                    ))
                    .build(),
            ),
        );
    }
}

/// The client API: topics, messages, tokens, who the caller is, the nodes and the statistics. Every route asks for a
/// token (with authentication on), except the probes.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Taba client API",
        description = "The API that applications and `taba-cli` use, on `api.port` (default 8080): topics, messages \
            (publish, consume, invisibility, confirm, prune), tokens, who the caller is, the nodes, the statistics \
            and, for an `admin` token, the cluster. A route's `Permission` is in its description; each one applies \
            only with authentication on. Errors are always an `ErrorResponse`, and every response carries \
            `X-Request-Id`.\n\nThe monitoring API (`/health/*`, `/metrics`, `/metrics/topics*` without a token, for the \
            collectors inside the infrastructure) is a separate document, served on `operations.port`."
    ),
    tags(
        (name = "Topics", description = "Queues and distributors."),
        (name = "Messages", description = "Publish, consume, change the invisibility, confirm and prune."),
        (name = "Tokens", description = "Access tokens. Only with authentication on."),
        (name = "Caller", description = "Who the token is."),
        (name = "Statistics", description = "The numbers of the topics, for the whole cluster."),
        (name = "Cluster", description = "The nodes and the cluster (`admin` permission)."),
        (name = "Health", description = "The probes of the orchestrator. They take no token."),
        (name = "Metrics", description = "This node's Prometheus metrics, behind a token on this port.")
    ),
    modifiers(&Common),
    security(("bearer" = []))
)]
struct ClientDoc;

/// The monitoring API: probes and metrics, with no token.
#[derive(OpenApi)]
#[openapi(
    info(
        title = "Taba monitoring API",
        description = "The routes for the collectors and the orchestrator **inside** the infrastructure (Prometheus, \
            Telegraf, Kubernetes probes), on `operations.port` (default 9090). They have **no authentication**: the \
            network and `operations.allowed-ips` protect them (`403 ops.ip-not-allowed` from outside the list). \
            Outside the infrastructure use the client API, which has `/metrics` behind a token.\n\nThe drain, the \
            S3 backup and the cluster-wide operations also live on this port but are not part of the published \
            contract."
    ),
    tags(
        (name = "Health", description = "Probes of the orchestrator."),
        (name = "Metrics", description = "Prometheus and per-topic metrics.")
    ),
    modifiers(&Common)
)]
struct MonitoringDoc;

/// The base of the client document, before the routes are registered.
pub fn client_base() -> utoipa::openapi::OpenApi {
    ClientDoc::openapi()
}

/// The base of the monitoring document, before the routes are registered.
pub fn monitoring_base() -> utoipa::openapi::OpenApi {
    MonitoringDoc::openapi()
}

/// `/docs` (the interface) and the document at `url`, both open.
pub fn docs(url: &'static str, spec: utoipa::openapi::OpenApi) -> Router<AppState> {
    SwaggerUi::new("/docs").url(url, spec).into()
}

/// One of the two published APIs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    Client,
    Monitoring,
}

impl Api {
    pub fn parse(name: &str) -> Option<Api> {
        match name {
            "client" => Some(Api::Client),
            "monitoring" => Some(Api::Monitoring),
            _ => None,
        }
    }
}

/// The document of an API as a node with authentication and Prometheus turned **on** (the defaults of a production
/// node) serves it, as indented JSON. This is what `taba openapi <api>` prints and what `openapi/*.json` holds.
pub fn document(api: Api) -> String {
    let spec = match api {
        Api::Client => super::client_api(true, true),
        Api::Monitoring => super::monitoring_api(true),
    }
    .into_openapi();
    spec.to_pretty_json()
        .expect("an OpenAPI document is always serializable")
}
