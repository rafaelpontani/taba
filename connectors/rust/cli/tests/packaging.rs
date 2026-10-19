//! The Docker image and the build scripts (doc 19b, sections 18.11.7 and 18.11.8). It is not possible to run
//! Docker or cross compilers in a unit test, so these tests check that the files
//! say what the documentation promises: the six targets, `--locked`, Alpine, the unprivileged
//! user and the `ENTRYPOINT` on the program. The real build is checked by hand and in CI.

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
        "first stage: Rust on Alpine"
    );
    assert!(
        docker.contains("\nFROM alpine:3\n"),
        "second stage: Alpine only"
    );
    assert!(
        docker.contains("+crt-static"),
        "static binary, linked against musl"
    );
    assert!(docker.contains("cargo build --release --locked -p taba-cli"));
    assert!(
        docker.contains("apk add --no-cache ca-certificates"),
        "the system's CA certificates"
    );
    assert!(
        docker.contains("adduser -D -u 10001 taba") && docker.contains("\nUSER taba\n"),
        "unprivileged"
    );
    assert!(
        docker.contains("ENTRYPOINT [\"taba-cli\"]"),
        "what comes after the image are the commands"
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
        assert!(
            script.contains(target),
            "build.sh without the target {target}"
        );
    }
    for option in ["--target", "--all"] {
        assert!(script.contains(option), "build.sh without {option}");
    }
    assert!(
        script.contains("--locked") && script.contains("SHA256SUMS") && script.contains("dist")
    );
    assert!(
        script.contains("cargo-zigbuild") && script.contains("cross"),
        "Linux aarch64 crosses with zigbuild or cross"
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
        assert!(
            script.contains(target),
            "build.ps1 without the target {target}"
        );
    }
    for option in ["$Target", "$All"] {
        assert!(script.contains(option), "build.ps1 without {option}");
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
    // On Linux only the two Linux targets are "reachable"; the macOS ones require a macOS.
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
        "  taba-0:",
        "  taba:",
        "  haproxy:",
        "  taba-cli:",
    ] {
        assert!(
            compose.contains(service),
            "docker-compose.yml without the service {service}"
        );
    }
    assert!(
        compose.contains("taba init --out /secrets"),
        "the init container runs taba init"
    );
    assert_eq!(
        compose.matches("TABA_CLUSTER_FIRST_NODE").count(),
        1,
        "only the first node starts the cluster"
    );
    assert!(
        compose.contains("TABA_CLUSTER_ENTRY_ADDRESS: haproxy:8081"),
        "discovery goes through HAProxy"
    );
    assert!(
        compose.contains("condition: service_healthy"),
        "the other nodes wait for the first to become ready"
    );
    assert!(
        compose.contains("profiles: [\"tools\"]"),
        "taba-cli stays in the tools profile"
    );
    assert!(compose.contains("TABA_CLI_URL: http://haproxy:8080"));
    assert!(compose.contains("TABA_CLI_TOKEN_FILE: /secrets/root-token"));
    assert!(
        compose.contains("stop_grace_period"),
        "time for the drain to finish before the SIGKILL"
    );
}

/// UC: UC-CLI-020
#[test]
fn the_compose_file_uses_public_images_and_the_dev_override_builds_them() {
    let compose = repo_file("docker-compose.yml");
    assert!(
        !compose.contains("build:"),
        "the root compose uses public images only; the build lives in the override"
    );
    assert!(compose.contains("image: rafaelpontani/taba:${TABA_VERSION:-latest}"));
    assert!(compose.contains("image: rafaelpontani/taba-cli:${TABA_VERSION:-latest}"));

    let dev = repo_file("docker-compose.dev.yml");
    for service in ["init", "taba-0", "taba", "taba-cli"] {
        let start = dev
            .find(&format!("\n  {service}:\n"))
            .unwrap_or_else(|| panic!("docker-compose.dev.yml without the service {service}"));
        // The override's services are separated by a blank line.
        let block = dev[start + 1..].split("\n\n").next().unwrap_or_default();
        assert!(
            block.contains("build:"),
            "the override builds the service {service}"
        );
    }
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
        "the cluster's mutual TLS must not be opened by the balancer"
    );
    assert!(
        cfg.contains("/health/ready"),
        "the API only receives ready nodes"
    );
    // The first node has a fixed name; the others are replicas of the `taba` service, discovered through DNS.
    for port in ["8080", "8081"] {
        assert!(cfg.contains(&format!("server taba-0 taba-0:{port}")));
        assert!(
            cfg.contains(&format!("server-template taba 1-10 taba:{port}")),
            "replicas enter rotation without listing each one"
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
