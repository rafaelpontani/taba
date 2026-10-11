//! A imagem Docker e os scripts de build (doc 19b, seções 18.11.7 e 18.11.8). Não dá para rodar o
//! Docker nem cruzar compiladores num teste unitário, então estes testes conferem que os arquivos
//! dizem o que a documentação promete: os seis alvos, o `--locked`, o Alpine, o usuário sem
//! privilégios e o `ENTRYPOINT` no programa. O build de verdade se confere à mão e no CI.

use std::fs;
use std::path::PathBuf;

fn cli_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

const TARGETS: [&str; 6] = [
    "x86_64-pc-windows-msvc",
    "aarch64-pc-windows-msvc",
    "x86_64-unknown-linux-musl",
    "aarch64-unknown-linux-musl",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];

/// UC: UC-CLI-020
#[test]
fn the_dockerfile_builds_a_static_binary_and_runs_it_as_an_unprivileged_user_on_alpine() {
    let docker = cli_file("Dockerfile");
    assert!(
        docker.contains("FROM rust:1-alpine AS build"),
        "primeira etapa: o Rust em Alpine"
    );
    assert!(
        docker.contains("\nFROM alpine:3\n"),
        "segunda etapa: só o Alpine"
    );
    assert!(
        docker.contains("+crt-static"),
        "binário estático, ligado à musl"
    );
    assert!(docker.contains("cargo build --release --locked -p taba-cli"));
    assert!(
        docker.contains("apk add --no-cache ca-certificates"),
        "os certificados de CA do sistema"
    );
    assert!(
        docker.contains("adduser -D -u 10001 taba") && docker.contains("\nUSER taba\n"),
        "sem privilégios"
    );
    assert!(
        docker.contains("ENTRYPOINT [\"taba-cli\"]"),
        "o que vem depois da imagem são os comandos"
    );
    assert!(docker.contains("COPY --from=build /taba-cli /usr/local/bin/taba-cli"));
}

/// UC: UC-CLI-020
#[test]
fn the_docker_script_offers_the_two_linux_platforms() {
    let script = cli_file("scripts/docker.sh");
    assert!(script.contains("linux/amd64,linux/arm64"));
    assert!(script.contains("docker buildx build"));
    assert!(script.contains("Dockerfile"));
}

/// UC: UC-CLI-021
#[test]
fn the_unix_script_covers_the_four_unix_targets_with_checksums_and_a_locked_build() {
    let script = cli_file("scripts/build.sh");
    for target in &TARGETS[2..] {
        assert!(script.contains(target), "build.sh sem o alvo {target}");
    }
    for option in ["--target", "--all"] {
        assert!(script.contains(option), "build.sh sem {option}");
    }
    assert!(
        script.contains("--locked") && script.contains("SHA256SUMS") && script.contains("dist")
    );
    assert!(
        script.contains("cargo-zigbuild") && script.contains("cross"),
        "o aarch64 do Linux cruza com o zigbuild ou o cross"
    );
    assert!(
        script.contains("CARGO_PROFILE_RELEASE_STRIP=symbols")
            && script.contains("CARGO_PROFILE_RELEASE_OPT_LEVEL=z")
    );
}

/// UC: UC-CLI-021
#[test]
fn the_windows_script_covers_the_two_windows_targets_with_checksums_and_a_locked_build() {
    let script = cli_file("scripts/build.ps1");
    for target in &TARGETS[..2] {
        assert!(script.contains(target), "build.ps1 sem o alvo {target}");
    }
    for option in ["$Target", "$All"] {
        assert!(script.contains(option), "build.ps1 sem {option}");
    }
    assert!(
        script.contains("--locked") && script.contains("SHA256SUMS") && script.contains("dist")
    );
    assert!(
        script.contains("CARGO_PROFILE_RELEASE_STRIP")
            && script.contains("CARGO_PROFILE_RELEASE_OPT_LEVEL")
    );
}

/// UC: UC-CLI-021
#[test]
fn the_scripts_do_not_try_to_cross_to_macos_from_another_system() {
    let sh = cli_file("scripts/build.sh");
    // No Linux só os dois alvos do Linux são "alcançáveis"; os do macOS exigem um macOS.
    assert!(sh.contains(
        "*-linux-*)      reachable=\"x86_64-unknown-linux-musl aarch64-unknown-linux-musl\""
    ));
    let ps = cli_file("scripts/build.ps1");
    assert!(ps.contains("$reachable = @('x86_64-pc-windows-msvc', 'aarch64-pc-windows-msvc')"));
}

fn repo_file(relative: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join(relative);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

/// UC: UC-CLI-020
#[test]
fn the_compose_file_has_the_init_container_three_nodes_haproxy_and_the_cli_tool() {
    let compose = repo_file("docker-compose.yml");
    for service in [
        "  init:",
        "  taba-1:",
        "  taba-2:",
        "  taba-3:",
        "  haproxy:",
        "  taba-cli:",
    ] {
        assert!(
            compose.contains(service),
            "docker-compose.yml sem o serviço {service}"
        );
    }
    assert!(
        compose.contains("taba init --out /secrets"),
        "o contêiner inicial roda o taba init"
    );
    assert_eq!(
        compose.matches("TABA_CLUSTER_FIRST_NODE").count(),
        1,
        "só o primeiro nó inicia o cluster"
    );
    assert!(
        compose.contains("TABA_CLUSTER_ENTRY_ADDRESS: haproxy:8081"),
        "a descoberta passa pelo HAProxy"
    );
    assert!(
        compose.contains("condition: service_healthy"),
        "os outros nós esperam o primeiro ficar pronto"
    );
    assert!(
        compose.contains("profiles: [\"tools\"]"),
        "o taba-cli fica no profile tools"
    );
    assert!(compose.contains("TABA_CLI_URL: http://haproxy:8080"));
    assert!(compose.contains("TABA_CLI_TOKEN_FILE: /secrets/root-token"));
    assert!(
        compose.contains("stop_grace_period"),
        "tempo para o drain terminar antes do SIGKILL"
    );
}

/// UC: UC-CLI-020
#[test]
fn the_haproxy_example_balances_the_client_api_and_passes_the_cluster_port_through_in_tcp() {
    let cfg = repo_file("docker/haproxy.cfg");
    assert!(cfg.contains("bind *:8080") && cfg.contains("bind *:8081"));
    let cluster = cfg
        .split("frontend cluster")
        .nth(1)
        .expect("frontend do cluster");
    assert!(
        cluster.contains("mode tcp"),
        "o TLS mútuo do cluster não pode ser aberto pelo balanceador"
    );
    assert!(cfg.contains("/health/ready"), "a API só recebe nós prontos");
    for node in ["taba-1", "taba-2", "taba-3"] {
        assert!(
            cfg.contains(&format!("server {node} {node}:8080"))
                && cfg.contains(&format!("server {node} {node}:8081"))
        );
    }
}

/// UC: UC-CLI-020
#[test]
fn the_node_image_runs_the_taba_binary_without_privileges() {
    let docker = repo_file("Dockerfile");
    assert!(docker.contains("cargo build --release --locked -p taba-node"));
    assert!(docker.contains("\nUSER taba\n") && docker.contains("ENTRYPOINT [\"taba\"]"));
    assert!(docker.contains("FROM alpine:3"));
}
