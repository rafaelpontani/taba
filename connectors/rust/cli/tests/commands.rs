//! The `taba-cli` commands end to end, minus the network: each test runs the whole program
//! (arguments, profiles, output, codes) over a fake transport and checks the call the CLI
//! builds (route, parameters, headers and body) and what it prints.

use std::collections::{HashMap, VecDeque};
use std::io::{self, Cursor};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use taba_cli::app::{run, Platform};
use taba_cli::profile::Os;
use taba_cli::tui::{Frame, Key, Terminal};
use taba_client::testing::{client, MockTransport, Seen};
use taba_client::transport::TransportError;
use taba_client::{Client, Config, Error as ApiError};

static COUNTER: AtomicUsize = AtomicUsize::new(0);

struct ScriptedTerminal {
    keys: VecDeque<Key>,
    frames: Arc<Mutex<Vec<Frame>>>,
}

impl Terminal for ScriptedTerminal {
    fn size(&self) -> (usize, usize) {
        (100, 30)
    }

    fn read_key(&mut self) -> io::Result<Option<Key>> {
        Ok(self.keys.pop_front())
    }

    fn draw(&mut self, frame: &Frame) -> io::Result<()> {
        self.frames.lock().unwrap().push(frame.clone());
        Ok(())
    }

    fn color(&self) -> bool {
        false
    }
}

struct TestPlatform {
    env: Mutex<HashMap<String, String>>,
    os: Os,
    tty: bool,
    mock: Arc<MockTransport>,
    exec_codes: Mutex<VecDeque<i32>>,
    exec_calls: Mutex<Vec<(String, Vec<u8>)>>,
    terminal: Mutex<Option<ScriptedTerminal>>,
}

impl Platform for TestPlatform {
    fn env(&self, name: &str) -> Option<String> {
        self.env.lock().unwrap().get(name).cloned()
    }

    fn os(&self) -> Os {
        self.os
    }

    fn stdin_is_tty(&self) -> bool {
        self.tty
    }

    fn stdout_is_tty(&self) -> bool {
        self.tty
    }

    fn client(&self, config: Config) -> Result<Client, ApiError> {
        Ok(client(config, &self.mock))
    }

    fn exec(&self, command: &str, stdin: &[u8]) -> io::Result<i32> {
        self.exec_calls
            .lock()
            .unwrap()
            .push((command.to_owned(), stdin.to_vec()));
        Ok(self.exec_codes.lock().unwrap().pop_front().unwrap_or(0))
    }

    fn terminal(&self) -> Option<Box<dyn Terminal>> {
        self.terminal
            .lock()
            .unwrap()
            .take()
            .map(|t| Box::new(t) as Box<dyn Terminal>)
    }
}

struct Out {
    code: i32,
    out: String,
    err: String,
}

struct Rig {
    dir: PathBuf,
    platform: TestPlatform,
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

impl Rig {
    fn new() -> Rig {
        let dir = std::env::temp_dir().join(format!(
            "taba-cli-it-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut env = HashMap::new();
        env.insert(
            "TABA_CLI_CONFIG".to_owned(),
            dir.join("config.yml").display().to_string(),
        );
        Rig {
            dir,
            platform: TestPlatform {
                env: Mutex::new(env),
                os: Os::Linux,
                tty: false,
                mock: MockTransport::new(),
                exec_codes: Mutex::new(VecDeque::new()),
                exec_calls: Mutex::new(Vec::new()),
                terminal: Mutex::new(None),
            },
        }
    }

    fn set_env(&self, name: &str, value: &str) {
        self.platform
            .env
            .lock()
            .unwrap()
            .insert(name.to_owned(), value.to_owned());
    }

    fn mock(&self) -> &Arc<MockTransport> {
        &self.platform.mock
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.yml")
    }

    fn file(&self, name: &str, content: &str) -> String {
        let path = self.dir.join(name);
        std::fs::write(&path, content).unwrap();
        path.display().to_string()
    }

    fn run(&self, args: &[&str]) -> Out {
        self.run_with(args, "")
    }

    fn run_with(&self, args: &[&str], stdin: &str) -> Out {
        let args: Vec<String> = args.iter().map(|s| s.to_string()).collect();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let mut input = Cursor::new(stdin.as_bytes().to_vec());
        let code = run(&self.platform, &args, &mut out, &mut err, &mut input);
        Out {
            code,
            out: String::from_utf8(out).unwrap(),
            err: String::from_utf8(err).unwrap(),
        }
    }

    fn seen(&self) -> Vec<Seen> {
        self.mock().seen()
    }
}

fn pretty(json: &str) -> String {
    taba_client::json::pretty(json).unwrap()
}

const TOPIC: &str = "{\"id\":\"01927f3a\",\"name\":\"orders\",\"type\":\"queue\",\"tags\":{},\"createdAt\":\"2026-10-06T12:00:00.000Z\",\"fifo\":false}";
const ERR_404: &str =
    "{\"code\":\"topic.not-found\",\"message\":\"The topic does not exist.\",\"requestId\":\"r-9\"}";

fn message(id: &str, body: &str) -> String {
    format!("{{\"id\":\"{id}\",\"body\":{body},\"contentType\":\"application/json\",\"receiptToken\":\"rt-{id}\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\"}}")
}

// ---------------------------------------------------------------------------------------------
// Connection, chosen profiles, output and errors
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-022, UC-CLI-001, UC-CLI-002
#[test]
fn without_any_configuration_it_calls_localhost_8080_without_a_token() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], TOPIC);
    let out = rig.run(&["topic", "get", "orders"]);
    assert_eq!(out.code, 0, "{}", out.err);
    let seen = rig.seen();
    assert_eq!(seen[0].origin, "http://127.0.0.1:8080");
    assert_eq!(
        (
            seen[0].request.method.as_str(),
            seen[0].request.target.as_str()
        ),
        ("GET", "/topics/orders")
    );
    assert_eq!(seen[0].header("Authorization"), None);
    assert!(seen[0]
        .header("User-Agent")
        .unwrap()
        .starts_with("taba-cli/"));
    assert!(
        !rig.config_path().exists(),
        "running a command does not create the profiles file"
    );
}

/// UC: UC-CLI-001, UC-CLI-002, UC-CLI-026
#[test]
fn the_default_profile_gives_the_url_and_the_token_from_its_file() {
    let rig = Rig::new();
    let token = rig.file("tok", "tb_from_file\nsecond line ignored\n");
    assert_eq!(
        rig.run(&[
            "profile",
            "add",
            "local",
            "--url",
            "http://10.0.0.7:8080",
            "--token-file",
            &token
        ])
        .code,
        0
    );
    rig.mock().reply(200, &[], TOPIC);
    assert_eq!(rig.run(&["topic", "get", "orders"]).code, 0);
    let seen = rig.seen();
    assert_eq!(seen[0].origin, "http://10.0.0.7:8080");
    assert_eq!(seen[0].header("Authorization"), Some("Bearer tb_from_file"));
}

/// UC: UC-CLI-026
#[test]
fn the_profile_flag_beats_the_variable_and_the_variable_overrides_only_its_field() {
    let rig = Rig::new();
    rig.run(&["profile", "add", "a", "--url", "http://a:1"]);
    rig.run(&[
        "profile",
        "add",
        "b",
        "--url",
        "http://b:2",
        "--timeout",
        "7",
    ]);
    rig.mock().reply(200, &[], TOPIC);
    rig.mock().reply(200, &[], TOPIC);
    rig.mock().reply(200, &[], TOPIC);
    rig.run(&["topic", "get", "x"]);
    rig.run(&["--profile", "b", "topic", "get", "x"]);
    rig.set_env("TABA_CLI_URL", "http://env:3");
    rig.run(&["topic", "get", "x", "--profile", "b"]);
    let seen = rig.seen();
    assert_eq!(seen[0].origin, "http://a:1");
    assert_eq!(seen[1].origin, "http://b:2");
    assert_eq!(seen[2].origin, "http://env:3");
    assert_eq!(
        seen[2].timeouts.total.as_secs(),
        7,
        "the rest comes from the chosen profile"
    );
}

/// UC: UC-CLI-026
#[test]
fn an_unknown_profile_is_a_usage_error() {
    let rig = Rig::new();
    let out = rig.run(&["--profile", "nope", "topic", "list"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("`nope` does not exist"), "{}", out.err);
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-002
#[test]
fn the_token_variable_works_without_any_profile() {
    let rig = Rig::new();
    rig.set_env("TABA_CLI_TOKEN", "tb_da_variavel");
    rig.mock().reply(200, &[], TOPIC);
    rig.run(&["topic", "get", "x"]);
    assert_eq!(
        rig.seen()[0].header("Authorization"),
        Some("Bearer tb_da_variavel")
    );
}

/// UC: UC-CLI-003, UC-CLI-005
#[test]
fn there_is_no_option_to_turn_off_certificate_checks_and_a_missing_ca_file_exits_5() {
    let rig = Rig::new();
    for flag in [
        "--insecure",
        "--no-verify",
        "--skip-tls-verify",
        "--url",
        "--token",
    ] {
        let out = rig.run(&["topic", "list", flag, "x"]);
        assert_eq!(out.code, 2, "{flag}");
        assert!(out.err.contains("unknown option"), "{}", out.err);
    }
    rig.set_env("TABA_CLI_CA_FILE", "/does/not/exist/ca.pem");
    let out = rig.run(&["topic", "list"]);
    assert_eq!(out.code, 5);
    assert!(out.err.contains("ca-file"), "{}", out.err);
}

/// UC: UC-CLI-005
#[test]
fn an_unreadable_token_file_exits_5() {
    let rig = Rig::new();
    rig.set_env("TABA_CLI_TOKEN_FILE", "/does/not/exist/tok");
    let out = rig.run(&["topic", "list"]);
    assert_eq!(out.code, 5);
    assert!(out.err.contains("token-file"), "{}", out.err);
    let empty = rig.file("empty", "\n");
    rig.set_env("TABA_CLI_TOKEN_FILE", &empty);
    assert_eq!(rig.run(&["topic", "list"]).code, 5);
}

/// UC: UC-CLI-004
#[test]
fn json_is_the_server_answer_indented_table_and_raw_change_only_the_form() {
    let rig = Rig::new();
    let page = format!("{{\"items\":[{TOPIC}]}}");
    for _ in 0..3 {
        rig.mock().reply(200, &[], &page);
    }
    let json = rig.run(&["topic", "list"]);
    assert_eq!(json.out, format!("{}\n", pretty(&page)));
    let table = rig.run(&["topic", "list", "-o", "table"]);
    assert!(
        table
            .out
            .starts_with("id        name    type   tags  createdAt"),
        "{}",
        table.out
    );
    assert!(table.out.contains("orders"));
    let raw = rig.run(&["topic", "list", "--output=raw"]);
    assert_eq!(raw.out, format!("{page}\n"));
}

/// UC: UC-CLI-004
#[test]
fn the_output_format_comes_from_the_profile_and_the_flag_overrides_it() {
    let rig = Rig::new();
    rig.run(&["profile", "add", "t", "--output", "table"]);
    let page = format!("{{\"items\":[{TOPIC}]}}");
    rig.mock().reply(200, &[], &page);
    rig.mock().reply(200, &[], &page);
    assert!(rig.run(&["topic", "list"]).out.starts_with("id "));
    assert!(rig
        .run(&["topic", "list", "-o", "json"])
        .out
        .starts_with("{\n"));
    assert_eq!(rig.run(&["topic", "list", "-o", "xml"]).code, 2);
}

/// UC: UC-CLI-004
#[test]
fn a_204_prints_nothing_and_quiet_silences_success() {
    let rig = Rig::new();
    rig.mock().reply(204, &[], "");
    let out = rig.run(&["topic", "delete", "orders", "--yes"]);
    assert_eq!((out.code, out.out.as_str(), out.err.as_str()), (0, "", ""));
    rig.mock().reply(200, &[], TOPIC);
    let quiet = rig.run(&["topic", "get", "orders", "-q"]);
    assert_eq!((quiet.code, quiet.out.as_str()), (0, ""));
}

/// UC: UC-CLI-005
#[test]
fn an_api_error_goes_to_stderr_with_code_message_and_request_id_and_exits_1() {
    let rig = Rig::new();
    rig.mock().reply(404, &[("X-Request-Id", "r-9")], ERR_404);
    let out = rig.run(&["topic", "get", "nope"]);
    assert_eq!(out.code, 1);
    assert_eq!(out.out, "");
    assert_eq!(
        out.err,
        "topic.not-found: The topic does not exist. (requestId r-9)\n"
    );
}

/// UC: UC-CLI-005
#[test]
fn with_json_the_error_response_is_printed_as_it_came_and_retry_after_gets_its_own_line() {
    let rig = Rig::new();
    rig.run(&["profile", "add", "p", "--max-attempts", "1"]);
    rig.mock().reply(
        503,
        &[("Retry-After", "5")],
        "{\"code\":\"cluster.quorum-unavailable\",\"message\":\"No quorum.\",\"requestId\":\"r\"}",
    );
    let out = rig.run(&["topic", "get", "x", "-o", "json"]);
    assert_eq!(out.code, 1);
    assert!(
        out.err
            .starts_with("{\n  \"code\": \"cluster.quorum-unavailable\""),
        "{}",
        out.err
    );
    assert!(out.err.contains("Retry-After: 5"), "{}", out.err);
}

/// UC: UC-CLI-005
#[test]
fn exit_codes_cover_usage_connection_and_timeout() {
    let rig = Rig::new();
    assert_eq!(rig.run(&["nope"]).code, 2);
    assert_eq!(rig.run(&["topic"]).code, 2);
    assert_eq!(rig.run(&["topic", "frobnicate"]).code, 2);
    assert_eq!(rig.run(&["topic", "get"]).code, 2, "the name is missing");
    assert_eq!(
        rig.run(&["topic", "get", "a", "b"]).code,
        2,
        "too many arguments"
    );
    assert_eq!(rig.run(&["topic", "get", "a", "--nope"]).code, 2);
    assert_eq!(rig.run(&[]).code, 0, "with no arguments it shows the help");

    rig.run(&["profile", "add", "p", "--max-attempts", "1"]);
    rig.mock().fail(TransportError::Connect("refused".into()));
    let out = rig.run(&["topic", "list"]);
    assert_eq!(out.code, 3);
    assert!(out.err.starts_with("client.network:"), "{}", out.err);

    rig.mock().fail(TransportError::Timeout { sent: true });
    let out = rig.run(&["topic", "list"]);
    assert_eq!(out.code, 4);
    assert!(out.err.starts_with("client.timeout:"), "{}", out.err);
}

/// UC: UC-CLI-005
#[test]
fn the_request_id_flag_goes_in_the_header() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], TOPIC);
    rig.run(&["topic", "get", "x", "--request-id", "meu-id"]);
    assert_eq!(rig.seen()[0].header("X-Request-Id"), Some("meu-id"));
}

// ---------------------------------------------------------------------------------------------
// Topics
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-006
#[test]
fn topic_create_sends_only_the_options_given() {
    let rig = Rig::new();
    rig.mock().reply(201, &[], TOPIC);
    rig.mock().reply(201, &[], TOPIC);
    let out = rig.run(&[
        "topic",
        "create",
        "orders",
        "--type",
        "queue",
        "--fifo",
        "--tag",
        "time=financeiro",
        "--tag",
        "a=b",
        "--ttl",
        "86400",
    ]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(out.out, format!("{}\n", pretty(TOPIC)));
    let seen = rig.seen();
    assert_eq!(
        (
            seen[0].request.method.as_str(),
            seen[0].request.target.as_str()
        ),
        ("POST", "/topics")
    );
    assert_eq!(seen[0].body_text(), "{\"name\":\"orders\",\"type\":\"queue\",\"tags\":{\"time\":\"financeiro\",\"a\":\"b\"},\"fifo\":true,\"ttl\":86400}");
    assert_eq!(seen[0].header("Content-Type"), Some("application/json"));

    rig.run(&[
        "topic",
        "create",
        "dist",
        "--type",
        "distributor",
        "--prefix",
        "orders-",
        "--prefix",
        "x",
    ]);
    assert_eq!(
        rig.seen()[1].body_text(),
        "{\"name\":\"dist\",\"type\":\"distributor\",\"prefixes\":[\"orders-\",\"x\"]}"
    );
}

/// UC: UC-CLI-006
#[test]
fn topic_create_needs_a_valid_type_and_well_formed_numbers_and_tags() {
    let rig = Rig::new();
    assert_eq!(rig.run(&["topic", "create", "x"]).code, 2);
    assert_eq!(
        rig.run(&["topic", "create", "x", "--type", "queue-ish"])
            .code,
        2
    );
    assert_eq!(
        rig.run(&["topic", "create", "x", "--type", "queue", "--ttl", "lots"])
            .code,
        2
    );
    assert_eq!(
        rig.run(&["topic", "create", "x", "--type", "queue", "--tag", "semigual"])
            .code,
        2
    );
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-006
#[test]
fn the_server_refusal_for_a_field_that_does_not_fit_the_type_is_shown() {
    let rig = Rig::new();
    rig.mock().reply(400, &[], "{\"code\":\"validation.not-applicable\",\"message\":\"A field that does not apply.\",\"requestId\":\"r\",\"details\":[{\"field\":\"fifo\",\"code\":\"validation.not-applicable\",\"message\":\"fifo does not apply to a distributor\"}]}");
    let out = rig.run(&["topic", "create", "d", "--type", "distributor", "--fifo"]);
    assert_eq!(out.code, 1);
    assert!(
        out.err.contains("validation.not-applicable")
            && out
                .err
                .contains("- fifo: fifo does not apply to a distributor"),
        "{}",
        out.err
    );
}

/// UC: UC-CLI-007
#[test]
fn topic_list_passes_name_sort_limit_and_cursor() {
    let rig = Rig::new();
    rig.mock()
        .reply(200, &[], "{\"items\":[],\"nextCursor\":\"c2\"}");
    let out = rig.run(&[
        "topic",
        "list",
        "--name",
        "order",
        "--sort=-createdAt",
        "--limit",
        "2",
        "--cursor",
        "c1",
    ]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        rig.seen()[0].request.target,
        "/topics?name=order&sort=-createdAt&limit=2&cursor=c1"
    );
    assert!(
        out.out.contains("\"nextCursor\": \"c2\""),
        "a page comes out as the server returned it: {}",
        out.out
    );
}

/// UC: UC-CLI-007
#[test]
fn topic_list_all_follows_every_page_and_prints_all_the_items() {
    let rig = Rig::new();
    rig.mock().reply(
        200,
        &[],
        "{\"items\":[{\"name\":\"a\"},{\"name\":\"b\"}],\"nextCursor\":\"p2\"}",
    );
    rig.mock().reply(
        200,
        &[],
        "{\"items\":[{\"name\":\"c\"}],\"nextCursor\":\"p3\"}",
    );
    rig.mock().reply(200, &[], "{\"items\":[{\"name\":\"d\"}]}");
    let out = rig.run(&["topic", "list", "--all", "-o", "raw"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        out.out,
        "{\"items\":[{\"name\":\"a\"},{\"name\":\"b\"},{\"name\":\"c\"},{\"name\":\"d\"}]}\n"
    );
    let targets: Vec<String> = rig
        .seen()
        .iter()
        .map(|s| s.request.target.clone())
        .collect();
    assert_eq!(
        targets,
        ["/topics", "/topics?cursor=p2", "/topics?cursor=p3"]
    );
}

/// UC: UC-CLI-008
#[test]
fn topic_get_and_update_send_only_what_was_given() {
    let rig = Rig::new();
    for _ in 0..4 {
        rig.mock().reply(200, &[], TOPIC);
    }
    rig.run(&[
        "topic",
        "update",
        "orders",
        "--ttl",
        "default",
        "--tag",
        "time=pagamentos",
    ]);
    rig.run(&[
        "topic",
        "update",
        "orders",
        "--clear-tags",
        "--max-deliveries",
        "5",
        "--max-message-bytes",
        "default",
    ]);
    rig.run(&[
        "topic", "update", "dist", "--prefix", "a-", "--prefix", "b-",
    ]);
    rig.run(&[
        "topic",
        "update",
        "dist",
        "--clear-prefixes",
        "--idle-timeout",
        "120",
    ]);
    let seen = rig.seen();
    assert_eq!(seen[0].request.method, "PATCH");
    assert_eq!(
        seen[0].body_text(),
        "{\"tags\":{\"time\":\"pagamentos\"},\"ttl\":null}"
    );
    assert_eq!(
        seen[1].body_text(),
        "{\"tags\":{},\"maxMessageBytes\":null,\"maxDeliveries\":5}"
    );
    assert_eq!(seen[2].body_text(), "{\"prefixes\":[\"a-\",\"b-\"]}");
    assert_eq!(seen[3].body_text(), "{\"prefixes\":[],\"idleTimeout\":120}");
    assert!(seen
        .iter()
        .all(|s| s.header("X-HTTP-Method-Override").is_none()));
}

/// UC: UC-CLI-008
#[test]
fn immutable_fields_are_not_options_and_an_empty_update_is_a_usage_error() {
    let rig = Rig::new();
    for flag in ["--name", "--type", "--volatile", "--fifo"] {
        assert_eq!(
            rig.run(&["topic", "update", "x", flag, "1"]).code,
            2,
            "{flag}"
        );
    }
    assert_eq!(rig.run(&["topic", "update", "x"]).code, 2);
    assert_eq!(
        rig.run(&["topic", "update", "x", "--clear-tags", "--tag", "a=b"])
            .code,
        2
    );
    assert_eq!(rig.run(&["topic", "update", "x", "--ttl", "abc"]).code, 2);
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-009
#[test]
fn delete_without_yes_refuses_outside_a_terminal() {
    let rig = Rig::new();
    let out = rig.run(&["topic", "delete", "orders"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("--yes"), "{}", out.err);
    assert!(rig.seen().is_empty());
    let out = rig.run(&["token", "revoke", "01927f3a"]);
    assert_eq!(out.code, 2);
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-009
#[test]
fn in_a_terminal_delete_asks_and_only_yes_goes_on() {
    let mut rig = Rig::new();
    rig.platform.tty = true;
    let out = rig.run_with(&["topic", "delete", "orders"], "n\n");
    assert_eq!(out.code, 2);
    assert!(
        out.err.contains("Remove the topic `orders`") && out.err.contains("Cancelled."),
        "{}",
        out.err
    );
    assert!(rig.seen().is_empty());

    rig.mock().reply(204, &[], "");
    let out = rig.run_with(&["topic", "delete", "orders"], "y\n");
    assert_eq!(out.code, 0, "{}", out.err);
    let seen = rig.seen();
    assert_eq!(
        (
            seen[0].request.method.as_str(),
            seen[0].request.target.as_str()
        ),
        ("DELETE", "/topics/orders")
    );

    rig.mock().reply(204, &[], "");
    assert_eq!(rig.run_with(&["token", "revoke", "t1"], "yes\n").code, 0);
    assert_eq!(rig.seen()[1].request.target, "/tokens/t1");
}

/// UC: UC-CLI-009
#[test]
fn yes_skips_the_question() {
    let rig = Rig::new();
    rig.mock().reply(204, &[], "");
    assert_eq!(rig.run(&["topic", "delete", "p", "--yes"]).code, 0);
    rig.mock().reply(204, &[], "");
    assert_eq!(rig.run(&["token", "revoke", "t", "--yes"]).code, 0);
}

// ---------------------------------------------------------------------------------------------
// Messages
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-010
#[test]
fn publish_takes_the_body_from_the_argument_the_file_or_stdin() {
    let rig = Rig::new();
    for _ in 0..3 {
        rig.mock()
            .reply(201, &[], "{\"id\":\"m1\",\"duplicate\":false}");
    }
    let from_arg = rig.run(&[
        "publish",
        "orders",
        "{\"a\":1}",
        "--group-id",
        "g 1",
        "--deduplication-id",
        "d-1",
    ]);
    assert_eq!(from_arg.code, 0, "{}", from_arg.err);
    assert_eq!(
        from_arg.out,
        format!("{}\n", pretty("{\"id\":\"m1\",\"duplicate\":false}"))
    );
    let path = rig.file("corpo.bin", "\u{1}\u{2}binario");
    rig.run(&[
        "publish",
        "orders",
        "--file",
        &path,
        "--content-type",
        "application/octet-stream",
    ]);
    rig.run_with(
        &["publish", "orders", "--stdin", "--text"],
        "line of text\n",
    );
    let seen = rig.seen();
    assert_eq!(
        seen[0].request.target,
        "/topics/orders/messages?groupId=g%201&deduplicationId=d-1"
    );
    assert_eq!(seen[0].body_text(), "{\"a\":1}");
    assert_eq!(seen[0].header("Content-Type"), Some("application/json"));
    assert_eq!(seen[1].request.body, "\u{1}\u{2}binario".as_bytes());
    assert_eq!(
        seen[1].header("Content-Type"),
        Some("application/octet-stream")
    );
    assert_eq!(seen[2].body_text(), "line of text\n");
    assert_eq!(
        seen[2].header("Content-Type"),
        Some("text/plain; charset=utf-8")
    );
}

/// UC: UC-CLI-010
#[test]
fn publish_wants_exactly_one_body_source_and_one_content_type_choice() {
    let rig = Rig::new();
    assert_eq!(rig.run(&["publish", "p"]).code, 2);
    assert_eq!(
        rig.run_with(&["publish", "p", "corpo", "--stdin"], "x")
            .code,
        2
    );
    assert_eq!(
        rig.run(&["publish", "p", "corpo", "--json", "--text"]).code,
        2
    );
    assert_eq!(
        rig.run(&["publish", "p", "corpo", "--json", "--content-type", "x/y"])
            .code,
        2
    );
    assert_eq!(rig.run(&["publish", "p", "corpo", "--count", "0"]).code, 2);
    let out = rig.run(&["publish", "p", "--file", "/does/not/exist.bin"]);
    assert_eq!(out.code, 5);
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-010
#[test]
fn publish_id_only_and_count() {
    let rig = Rig::new();
    rig.mock().reply(201, &[], "{\"id\":\"m1\"}");
    let out = rig.run(&["publish", "p", "oi", "--text", "--id-only"]);
    assert_eq!(out.out, "m1\n");
    rig.mock().reply(201, &[], "{\"id\":\"a\"}");
    rig.mock().reply(201, &[], "{\"id\":\"b\"}");
    rig.mock().reply(201, &[], "{\"id\":\"c\"}");
    let out = rig.run(&["publish", "p", "{}", "--count", "3", "--id-only"]);
    assert_eq!(out.out, "a\nb\nc\n");
    assert_eq!(rig.seen().len(), 4);
}

/// UC: UC-CLI-010
#[test]
fn publish_does_not_validate_the_body_the_server_does() {
    let rig = Rig::new();
    rig.mock().reply(
        400,
        &[],
        "{\"code\":\"validation.invalid-body\",\"message\":\"Invalid JSON.\",\"requestId\":\"r\"}",
    );
    let out = rig.run(&["publish", "p", "this is not json"]);
    assert_eq!(out.code, 1);
    assert_eq!(rig.seen()[0].body_text(), "this is not json");
    assert!(out.err.starts_with("validation.invalid-body:"));
}

/// UC: UC-CLI-010
#[test]
fn a_distributor_answer_is_printed_whole_even_with_id_only() {
    let rig = Rig::new();
    let answer = "{\"matched\":1,\"deliveries\":[{\"topic\":\"a\",\"id\":\"x\"}]}";
    rig.mock().reply(200, &[], answer);
    let out = rig.run(&["publish", "dist", "{}", "--id-only", "-o", "raw"]);
    assert_eq!(out.out, format!("{answer}\n"));
}

/// UC: UC-CLI-011
#[test]
fn after_a_502_outside_a_fifo_publish_is_not_repeated() {
    let rig = Rig::new();
    let gateway = "{\"code\":\"forward.bad-gateway\",\"message\":\"x\",\"requestId\":\"r\"}";
    rig.mock().reply(502, &[], gateway);
    rig.mock().reply(200, &[], TOPIC); // the CLI looks at the topic: it is not FIFO
    let out = rig.run(&["publish", "queue", "{}"]);
    assert_eq!(out.code, 1);
    let seen = rig.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].request.method, "POST");
    assert_eq!(seen[1].request.target, "/topics/queue");
}

/// UC: UC-CLI-011
#[test]
fn after_a_502_on_a_fifo_publish_is_repeated_because_the_primary_deduplicates() {
    let rig = Rig::new();
    let gateway = "{\"code\":\"forward.bad-gateway\",\"message\":\"x\",\"requestId\":\"r\"}";
    rig.mock().reply(504, &[], gateway);
    rig.mock()
        .reply(200, &[], &TOPIC.replace("\"fifo\":false", "\"fifo\":true"));
    rig.mock()
        .reply(201, &[], "{\"id\":\"m1\",\"duplicate\":true}");
    let out = rig.run(&["publish", "queue", "{}", "--group-id", "g"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.out.contains("\"duplicate\": true"));
    assert_eq!(
        rig.seen()
            .iter()
            .filter(|s| s.request.method == "POST")
            .count(),
        2
    );
}

/// UC: UC-CLI-012
#[test]
fn consume_sends_only_the_given_parameters_and_prints_the_whole_message_with_the_receipt() {
    let rig = Rig::new();
    let m = message("m1", "{\"order\":9001}");
    rig.mock()
        .reply(200, &[], &format!("{{\"messages\":[{m}]}}"));
    let out = rig.run(&["consume", "orders"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(rig.seen()[0].request.target, "/topics/orders/messages");
    assert_eq!(
        out.out,
        format!("{}\n", pretty(&format!("{{\"messages\":[{m}]}}")))
    );
    assert!(out.out.contains("\"receiptToken\": \"rt-m1\""));

    rig.mock().reply(200, &[], "{\"messages\":[]}");
    rig.run(&[
        "consume",
        "orders",
        "--max",
        "5",
        "--visibility",
        "60",
        "--wait",
        "20",
        "--auto-create",
        "--persistent",
        "--idle-timeout",
        "900",
        "--fifo",
        "--tag",
        "time=fin",
    ]);
    let seen = rig.seen();
    assert_eq!(
        seen[1].request.target,
        "/topics/orders/messages?maxMessages=5&visibilityTimeout=60&waitTime=20&autoCreate=true&volatile=false&idleTimeout=900&fifo=true&tag.time=fin"
    );
    assert_eq!(seen[1].timeouts.total.as_secs(), 30, "waitTime 20 + 10 s");
}

/// UC: UC-CLI-012
#[test]
fn the_auto_create_options_need_auto_create() {
    let rig = Rig::new();
    for extra in [
        ["--persistent", ""],
        ["--fifo", ""],
        ["--idle-timeout", "9"],
        ["--tag", "a=b"],
    ] {
        let mut args = vec!["consume", "f", extra[0]];
        if !extra[1].is_empty() {
            args.push(extra[1]);
        }
        assert_eq!(rig.run(&args).code, 2, "{extra:?}");
    }
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-012
#[test]
fn consume_raw_prints_only_the_body_one_per_line_and_ack_confirms_each_message() {
    let rig = Rig::new();
    let (a, b) = (message("m1", "{\"x\":1}"), message("m2", "{\"x\":2}"));
    rig.mock()
        .reply(200, &[], &format!("{{\"messages\":[{a},{b}]}}"));
    rig.mock().reply(204, &[], "");
    rig.mock().reply(204, &[], "");
    let out = rig.run(&["consume", "orders", "--max", "2", "--ack", "-o", "raw"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(out.out, "{\"x\":1}\n{\"x\":2}\n");
    let seen = rig.seen();
    assert_eq!(
        (
            seen[1].request.method.as_str(),
            seen[1].request.target.as_str()
        ),
        ("DELETE", "/topics/orders/messages/m1?receiptToken=rt-m1")
    );
    assert_eq!(
        seen[2].request.target,
        "/topics/orders/messages/m2?receiptToken=rt-m2"
    );
}

/// UC: UC-CLI-012
#[test]
fn exit_empty_returns_6_only_when_nothing_came() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], "{\"messages\":[]}");
    let out = rig.run(&["consume", "f", "--exit-empty"]);
    assert_eq!(out.code, 6);
    assert_eq!(out.out.trim(), "{\n  \"messages\": []\n}");
    rig.mock().reply(200, &[], "{\"messages\":[]}");
    assert_eq!(
        rig.run(&["consume", "f"]).code,
        0,
        "without --exit-empty, empty is success"
    );
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m", "1")),
    );
    assert_eq!(rig.run(&["consume", "f", "--exit-empty"]).code, 0);
}

/// UC: UC-CLI-012
#[test]
fn consume_binary_bodies_come_out_decoded_in_raw_mode() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], "{\"messages\":[{\"id\":\"m\",\"body\":\"Q2Fmw6k=\",\"contentType\":\"application/octet-stream\",\"bodyEncoding\":\"base64\",\"receiptToken\":\"r\",\"deliveryCount\":1,\"publishedAt\":\"p\",\"invisibleUntil\":\"i\"}]}");
    let out = rig.run(&["consume", "f", "-o", "raw"]);
    assert_eq!(out.out, "Café\n");
}

/// UC: UC-CLI-013
#[test]
fn subscribe_prints_each_message_confirms_it_and_stops_at_the_limit() {
    let rig = Rig::new();
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m1", "{\"n\":1}")),
    );
    rig.mock().reply(204, &[], "");
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m2", "{\"n\":2}")),
    );
    rig.mock().reply(204, &[], "");
    let out = rig.run(&["subscribe", "orders", "--limit", "2", "-o", "raw"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(out.out, "{\"n\":1}\n{\"n\":2}\n");
    let seen = rig.seen();
    assert_eq!(
        seen[0].request.target,
        "/topics/orders/messages?maxMessages=1&waitTime=20"
    );
    assert_eq!(
        seen.iter().filter(|s| s.request.method == "DELETE").count(),
        2
    );
}

/// UC: UC-CLI-013
#[test]
fn subscribe_exec_runs_the_command_with_the_body_and_confirms_only_on_exit_zero() {
    let rig = Rig::new();
    rig.platform.exec_codes.lock().unwrap().extend([0, 3]);
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m1", "{\"n\":1}")),
    );
    rig.mock().reply(204, &[], "");
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m2", "{\"n\":2}")),
    );
    let out = rig.run(&[
        "subscribe",
        "orders",
        "--exec",
        "./process.sh",
        "--limit",
        "2",
        "--wait",
        "5",
        "--max",
        "1",
        "--visibility",
        "30",
    ]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        out.out, "",
        "with --exec the CLI does not print the message"
    );
    let calls = rig.platform.exec_calls.lock().unwrap().clone();
    assert_eq!(
        calls,
        [
            ("./process.sh".to_owned(), b"{\"n\":1}".to_vec()),
            ("./process.sh".to_owned(), b"{\"n\":2}".to_vec())
        ]
    );
    let seen = rig.seen();
    let acks: Vec<&str> = seen
        .iter()
        .filter(|s| s.request.method == "DELETE")
        .map(|s| s.request.target.as_str())
        .collect();
    assert_eq!(
        acks,
        ["/topics/orders/messages/m1?receiptToken=rt-m1"],
        "m2 exited with 3 and is not acknowledged"
    );
    assert_eq!(
        seen[0].request.target,
        "/topics/orders/messages?maxMessages=1&visibilityTimeout=30&waitTime=5"
    );
}

/// UC: UC-CLI-013
#[test]
fn subscribe_keeps_going_when_the_receipt_went_stale_during_processing() {
    let rig = Rig::new();
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m1", "1")),
    );
    rig.mock().reply(
        409,
        &[],
        "{\"code\":\"message.stale-receipt\",\"message\":\"x\",\"requestId\":\"r\"}",
    );
    rig.mock().reply(
        200,
        &[],
        &format!("{{\"messages\":[{}]}}", message("m2", "2")),
    );
    rig.mock().reply(204, &[], "");
    let out = rig.run(&["subscribe", "orders", "--limit", "2", "-o", "raw"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(out.out, "1\n2\n");
}

/// UC: UC-CLI-014
#[test]
fn ack_and_visibility_need_the_receipt_and_visibility_needs_seconds() {
    let rig = Rig::new();
    assert_eq!(rig.run(&["ack", "p", "m1"]).code, 2);
    assert_eq!(
        rig.run(&["visibility", "p", "m1", "--receipt", "r"]).code,
        2
    );
    assert_eq!(
        rig.run(&["visibility", "p", "m1", "--seconds", "5"]).code,
        2
    );
    assert!(rig.seen().is_empty());

    rig.mock().reply(204, &[], "");
    rig.mock().reply(
        200,
        &[],
        "{\"id\":\"m1\",\"invisibleUntil\":\"2026-10-06T12:02:30.000Z\"}",
    );
    assert_eq!(rig.run(&["ack", "p", "m1", "--receipt", "rt 1"]).out, "");
    let out = rig.run(&[
        "visibility",
        "p",
        "m1",
        "--receipt",
        "rt1",
        "--seconds",
        "0",
    ]);
    assert_eq!(
        out.out,
        format!(
            "{}\n",
            pretty("{\"id\":\"m1\",\"invisibleUntil\":\"2026-10-06T12:02:30.000Z\"}")
        )
    );
    let seen = rig.seen();
    assert_eq!(
        seen[0].request.target,
        "/topics/p/messages/m1?receiptToken=rt%201"
    );
    assert_eq!(
        (
            seen[1].request.method.as_str(),
            seen[1].request.target.as_str()
        ),
        ("PUT", "/topics/p/messages/m1/visibility?receiptToken=rt1")
    );
    assert_eq!(seen[1].body_text(), "{\"visibilityTimeout\":0}");
}

/// UC: UC-CLI-014
#[test]
fn a_stale_receipt_is_a_409_error_with_exit_1() {
    let rig = Rig::new();
    rig.mock().reply(
        409,
        &[],
        "{\"code\":\"message.stale-receipt\",\"message\":\"Token obsoleto.\",\"requestId\":\"r\"}",
    );
    let out = rig.run(&["ack", "p", "m1", "--receipt", "old"]);
    assert_eq!(out.code, 1);
    assert!(out.err.starts_with("message.stale-receipt:"), "{}", out.err);
}

// ---------------------------------------------------------------------------------------------
// Tokens
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-015
#[test]
fn token_create_sends_name_permissions_prefixes_and_expiry() {
    let rig = Rig::new();
    let created = "{\"id\":\"t1\",\"token\":\"tb_t1.segredo\",\"name\":\"servico\",\"permissions\":[\"publish\",\"create\"]}";
    rig.mock().reply(201, &[], created);
    rig.mock().reply(201, &[], created);
    let out = rig.run(&[
        "token",
        "create",
        "--name",
        "servico-orders",
        "--permission",
        "publish,create",
        "--permission",
        "consume",
        "--prefix",
        "orders",
        "--expires-at",
        "2027-06-01T00:00:00.000Z",
    ]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.out.contains("\"token\": \"tb_t1.segredo\""));
    assert_eq!(
        rig.seen()[0].body_text(),
        "{\"name\":\"servico-orders\",\"permissions\":[\"publish\",\"create\",\"consume\"],\"topicPrefixes\":[\"orders\"],\"expiresAt\":\"2027-06-01T00:00:00.000Z\"}"
    );
    let only = rig.run(&[
        "token",
        "create",
        "--name",
        "x",
        "--permission",
        "admin",
        "--token-only",
    ]);
    assert_eq!(only.out, "tb_t1.segredo\n");
}

/// UC: UC-CLI-015
#[test]
fn token_create_validates_the_permissions_without_the_server() {
    let rig = Rig::new();
    assert_eq!(
        rig.run(&["token", "create", "--permission", "admin"]).code,
        2
    );
    assert_eq!(rig.run(&["token", "create", "--name", "x"]).code, 2);
    assert_eq!(
        rig.run(&["token", "create", "--name", "x", "--permission", "root"])
            .code,
        2
    );
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-015
#[test]
fn token_list_get_and_update_use_the_token_routes() {
    let rig = Rig::new();
    rig.mock()
        .reply(200, &[], "{\"items\":[{\"id\":\"t1\",\"name\":\"a\"}]}");
    rig.mock().reply(200, &[], "{\"id\":\"t1\",\"name\":\"a\"}");
    rig.mock().reply(200, &[], "{\"id\":\"t1\",\"name\":\"b\"}");
    rig.mock().reply(200, &[], "{\"id\":\"t1\",\"name\":\"b\"}");
    rig.run(&[
        "token",
        "list",
        "--name",
        "a",
        "--sort",
        "createdAt",
        "--limit",
        "5",
    ]);
    rig.run(&["token", "get", "t1"]);
    rig.run(&[
        "token",
        "update",
        "t1",
        "--name",
        "b",
        "--expires-at",
        "2027-01-01T00:00:00Z",
    ]);
    rig.run(&["token", "update", "t1", "--expires-at", "none"]);
    let seen = rig.seen();
    assert_eq!(
        seen[0].request.target,
        "/tokens?name=a&sort=createdAt&limit=5"
    );
    assert_eq!(seen[1].request.target, "/tokens/t1");
    assert_eq!(seen[2].request.method, "PATCH");
    assert_eq!(
        seen[2].body_text(),
        "{\"name\":\"b\",\"expiresAt\":\"2027-01-01T00:00:00Z\"}"
    );
    assert_eq!(seen[3].body_text(), "{\"expiresAt\":null}");
    assert_eq!(rig.run(&["token", "update", "t1"]).code, 2);
}

/// UC: UC-CLI-015
#[test]
fn without_authentication_the_tokens_routes_answer_404_and_the_cli_says_so() {
    let rig = Rig::new();
    rig.mock().reply(
        404,
        &[],
        "{\"code\":\"route.not-found\",\"message\":\"Rota inexistente.\",\"requestId\":\"r\"}",
    );
    let out = rig.run(&["token", "list"]);
    assert_eq!(out.code, 1);
    assert!(out.err.starts_with("route.not-found:"), "{}", out.err);
}

// ---------------------------------------------------------------------------------------------
// Operations
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-016
#[test]
fn health_uses_the_ops_url_without_a_token_and_prints_the_body() {
    let rig = Rig::new();
    rig.set_env("TABA_CLI_TOKEN", "tb_nao_deve_ir");
    rig.set_env("TABA_CLI_URL", "http://taba-1:8080");
    rig.mock().reply(200, &[], "{\"health\":\"live\"}");
    rig.mock()
        .reply(200, &[], "{\"scope\":\"node\",\"nodes\":[]}");
    let live = rig.run(&["health", "live"]);
    assert_eq!(
        (live.code, live.out.as_str()),
        (0, "{\n  \"health\": \"live\"\n}\n")
    );
    rig.run(&["health", "cluster", "--scope", "node"]);
    let seen = rig.seen();
    assert_eq!(seen[0].origin, "http://taba-1:8080");
    assert_eq!(seen[0].request.target, "/health/live");
    assert_eq!(seen[0].header("Authorization"), None);
    assert_eq!(seen[1].request.target, "/health/cluster?scope=node");
}

/// UC: UC-CLI-016
#[test]
fn health_ready_shows_the_reason_on_a_503_and_exits_1_without_retrying() {
    let rig = Rig::new();
    rig.mock().reply(
        503,
        &[],
        "{\"health\":\"not-ready\",\"reason\":\"the node is syncing\"}",
    );
    let out = rig.run(&["health", "ready"]);
    assert_eq!(out.code, 1);
    assert!(
        out.out.contains("\"reason\": \"the node is syncing\""),
        "{}",
        out.out
    );
    assert_eq!(rig.seen().len(), 1, "a health 503 is not retried");
}

/// UC: UC-CLI-016
#[test]
fn health_cluster_validates_the_scope() {
    let rig = Rig::new();
    assert_eq!(
        rig.run(&["health", "cluster", "--scope", "galaxia"]).code,
        2
    );
    assert!(rig.seen().is_empty());
}

/// UC: UC-CLI-016
#[test]
fn health_live_with_nothing_listening_exits_3() {
    let rig = Rig::new();
    rig.mock().fail(TransportError::Connect("refused".into()));
    assert_eq!(rig.run(&["health", "live"]).code, 3);
    assert_eq!(rig.seen().len(), 1);
}

/// UC: UC-CLI-017
#[test]
fn metrics_prints_the_text_as_it_came_and_openmetrics_changes_the_accept_header() {
    let rig = Rig::new();
    rig.mock().reply(
        200,
        &[("Content-Type", "text/plain")],
        "# HELP taba_x x\ntaba_x 1\n",
    );
    rig.mock().reply(200, &[], "# EOF\n");
    let out = rig.run(&["metrics", "-o", "table"]);
    assert_eq!(out.out, "# HELP taba_x x\ntaba_x 1\n");
    rig.run(&["metrics", "--openmetrics"]);
    let seen = rig.seen();
    assert_eq!(
        (seen[0].origin.as_str(), seen[0].request.target.as_str()),
        ("http://127.0.0.1:8080", "/metrics")
    );
    assert_eq!(seen[0].header("Accept"), Some("text/plain"));
    assert_eq!(
        seen[1].header("Accept"),
        Some("application/openmetrics-text")
    );
}

/// UC: UC-CLI-017
#[test]
fn stats_pass_scope_tags_condition_fields_group_by_and_pagination() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], "{\"items\":[]}");
    rig.mock().reply(200, &[], "{\"id\":\"i\"}");
    rig.mock().reply(
        200,
        &[],
        "{\"items\":[{\"group\":\"g1\"}],\"nextCursor\":\"n\"}",
    );
    rig.mock()
        .reply(200, &[], "{\"items\":[{\"group\":\"g2\"}]}");
    rig.run(&[
        "stats",
        "topics",
        "--scope",
        "node",
        "--tag",
        "time=fin",
        "--tag",
        "env=prod",
        "--condition",
        "backlog",
        "--fields",
        "name,depth",
        "--group-by",
        "time",
        "--limit",
        "10",
        "--cursor",
        "c",
    ]);
    rig.run(&["stats", "topic", "orders", "--scope", "cluster"]);
    let all = rig.run(&["stats", "groups", "orders", "--all", "-o", "raw"]);
    let seen = rig.seen();
    assert_eq!(seen[0].request.target, "/metrics/topics?scope=node&tag.time=fin&tag.env=prod&condition=backlog&fields=name%2Cdepth&groupBy=time&limit=10&cursor=c");
    assert_eq!(
        seen[1].request.target,
        "/metrics/topics/orders?scope=cluster"
    );
    assert_eq!(seen[2].request.target, "/metrics/topics/orders/groups");
    assert_eq!(
        seen[3].request.target,
        "/metrics/topics/orders/groups?cursor=n"
    );
    assert_eq!(
        all.out,
        "{\"items\":[{\"group\":\"g1\"},{\"group\":\"g2\"}]}\n"
    );
}

/// UC: UC-CLI-018
#[test]
fn drain_acts_on_the_node_of_the_profile_ops_url() {
    let rig = Rig::new();
    rig.run(&[
        "profile",
        "add",
        "no2",
        "--url",
        "http://n2:8080",
        "--ops-url",
        "http://n2:9191",
    ]);
    for _ in 0..3 {
        rig.mock().reply(202, &[], "{\"state\":\"draining\"}");
    }
    let start = rig.run(&["drain", "start"]);
    assert_eq!(
        (start.code, start.out.as_str()),
        (0, "{\n  \"state\": \"draining\"\n}\n")
    );
    rig.run(&["drain", "status"]);
    rig.run(&["drain", "cancel"]);
    let seen = rig.seen();
    let routes: Vec<(&str, &str, &str)> = seen
        .iter()
        .map(|s| {
            (
                s.origin.as_str(),
                s.request.method.as_str(),
                s.request.target.as_str(),
            )
        })
        .collect();
    assert_eq!(
        routes,
        [
            ("http://n2:9191", "POST", "/node/drain"),
            ("http://n2:9191", "GET", "/node/drain"),
            ("http://n2:9191", "DELETE", "/node/drain"),
        ]
    );
}

// ---------------------------------------------------------------------------------------------
// Help and version
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-019
#[test]
fn version_shows_the_program_and_the_contract_version() {
    let rig = Rig::new();
    for args in [
        &["version"][..],
        &["--version"],
        &["topic", "list", "--version"],
    ] {
        let out = rig.run(args);
        assert_eq!(out.code, 0);
        // The version comes from Cargo.toml: on release, set-version replaces it with the tag version.
        let expected = format!("taba-cli {} (API contract 1)", env!("CARGO_PKG_VERSION"));
        assert!(out.out.starts_with(&expected), "{}", out.out);
    }
}

/// UC: UC-CLI-019
#[test]
fn help_lists_the_commands_and_shows_each_one() {
    let rig = Rig::new();
    let all = rig.run(&["help"]);
    for word in [
        "topic create",
        "publish",
        "consume",
        "subscribe",
        "token create",
        "health cluster",
        "drain start",
        "profile add",
        "config show",
    ] {
        assert!(
            all.out.contains(word),
            "faltou {word} na ajuda:\n{}",
            all.out
        );
    }
    let one = rig.run(&["help", "topic", "create"]);
    assert!(
        one.out
            .contains("usage: taba-cli topic create <name> [options]")
            && one.out.contains("--type <value>"),
        "{}",
        one.out
    );
    let flagged = rig.run(&["topic", "create", "--help"]);
    assert_eq!(flagged.out, one.out);
    let group = rig.run(&["help", "topic"]);
    assert!(group.out.contains("topic list") && group.out.contains("topic delete"));
    assert!(rig.run(&["help", "nope"]).out.contains("There is no help"));
    assert!(rig.seen().is_empty());
}

// ---------------------------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------------------------

/// UC: UC-CLI-023
#[test]
fn profile_path_prints_the_config_file_location() {
    let rig = Rig::new();
    let out = rig.run(&["profile", "path"]);
    assert_eq!(out.out.trim(), rig.config_path().display().to_string());
}

/// UC: UC-CLI-023
#[test]
fn the_default_location_follows_the_system_when_the_override_is_absent() {
    let rig = Rig::new();
    rig.platform.env.lock().unwrap().remove("TABA_CLI_CONFIG");
    rig.set_env("HOME", "/home/ana");
    let out = rig.run(&["profile", "path"]);
    let expected = std::path::Path::new("/home/ana")
        .join(".config")
        .join("taba")
        .join("config.yml");
    assert_eq!(out.out.trim(), expected.display().to_string());
}

/// UC: UC-CLI-024
#[test]
fn profile_add_creates_the_file_the_first_profile_is_the_default_and_a_duplicate_exits_2() {
    let rig = Rig::new();
    let out = rig.run(&["profile", "add", "local", "--url", "http://127.0.0.1:8080"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert_eq!(
        rig.run(&["profile", "add", "stg", "--url", "https://stg:8080"])
            .code,
        0
    );
    let text = std::fs::read_to_string(rig.config_path()).unwrap();
    assert!(
        text.contains("default: local") && text.contains("stg:"),
        "{text}"
    );
    let dup = rig.run(&["profile", "add", "stg"]);
    assert_eq!(dup.code, 2);
    assert!(dup.err.contains("already exists"), "{}", dup.err);
    assert_eq!(rig.run(&["profile", "add", "with space"]).code, 2);
    assert_eq!(
        rig.run(&["profile", "add", "x", "--url", "no-scheme"]).code,
        2
    );
    assert_eq!(rig.run(&["profile", "add", "y", "--default"]).code, 0);
    assert_eq!(rig.run(&["profile", "default"]).out.trim(), "y");
}

/// UC: UC-CLI-024
#[test]
fn profile_set_changes_fields_and_unset_clears_them() {
    let rig = Rig::new();
    rig.run(&[
        "profile",
        "add",
        "p",
        "--url",
        "http://a:1",
        "--timeout",
        "60",
        "--output",
        "table",
    ]);
    assert_eq!(
        rig.run(&[
            "profile",
            "set",
            "p",
            "--url",
            "http://b:2",
            "--unset",
            "timeout",
            "--unset",
            "output"
        ])
        .code,
        0
    );
    let shown = rig.run(&["profile", "show", "p", "-o", "raw"]);
    assert_eq!(
        shown.out.trim(),
        "{\"name\":\"p\",\"default\":true,\"url\":\"http://b:2\"}"
    );
    assert_eq!(
        rig.run(&["profile", "set", "nope", "--url", "http://x:1"])
            .code,
        2
    );
    assert_eq!(rig.run(&["profile", "set", "p"]).code, 2);
    assert_eq!(rig.run(&["profile", "set", "p", "--unset", "cor"]).code, 2);
    assert_eq!(rig.run(&["profile", "set", "p", "--timeout", "0"]).code, 2);
}

/// UC: UC-CLI-024
#[test]
fn profile_remove_asks_outside_yes_and_removing_the_default_leaves_none() {
    let mut rig = Rig::new();
    rig.run(&["profile", "add", "a"]);
    rig.run(&["profile", "add", "b"]);
    assert_eq!(
        rig.run(&["profile", "remove", "a"]).code,
        2,
        "off a terminal it requires --yes"
    );
    assert_eq!(rig.run(&["profile", "remove", "nope", "--yes"]).code, 2);
    assert_eq!(rig.run(&["profile", "remove", "a", "--yes"]).code, 0);
    assert_eq!(rig.run(&["profile", "default"]).out.trim(), "(none)");
    rig.platform.tty = true;
    assert_eq!(rig.run_with(&["profile", "remove", "b"], "n\n").code, 2);
    assert_eq!(rig.run_with(&["profile", "remove", "b"], "y\n").code, 0);
    let list = rig.run(&["profile", "list", "-o", "raw"]);
    assert_eq!(list.out.trim(), "{\"items\":[]}");
}

/// UC: UC-CLI-025
#[test]
fn profile_default_shows_sets_and_clears_and_list_marks_default_and_session() {
    let rig = Rig::new();
    rig.run(&["profile", "add", "a", "--url", "http://a:1"]);
    rig.run(&[
        "profile",
        "add",
        "b",
        "--url",
        "http://b:2",
        "--token",
        "tb_segredo_nao_aparece",
    ]);
    assert_eq!(rig.run(&["profile", "default", "b"]).code, 0);
    assert_eq!(rig.run(&["profile", "default", "nope"]).code, 2);
    rig.set_env("TABA_CLI_PROFILE", "a");
    let list = rig.run(&["profile", "list", "-o", "raw"]);
    assert_eq!(
        list.out.trim(),
        "{\"items\":[{\"name\":\"a\",\"default\":false,\"url\":\"http://a:1\",\"session\":true},{\"name\":\"b\",\"default\":true,\"url\":\"http://b:2\",\"token\":\"(set)\"}]}"
    );
    assert!(!list.out.contains("tb_segredo"));
    let table = rig.run(&["profile", "list", "-o", "table"]);
    assert!(
        table.out.lines().next().unwrap().starts_with("name"),
        "{}",
        table.out
    );
    assert_eq!(rig.run(&["profile", "default", "--none"]).code, 0);
    assert_eq!(rig.run(&["profile", "default"]).out.trim(), "(none)");
}

/// UC: UC-CLI-025
#[test]
fn profile_show_never_prints_the_token() {
    let rig = Rig::new();
    rig.run(&[
        "profile",
        "add",
        "p",
        "--token",
        "tb_segredo_nao_aparece",
        "--token-file",
        "/sec/tok",
    ]);
    for args in [
        &["profile", "show", "p"][..],
        &["profile", "show"],
        &["config", "show"],
    ] {
        let out = rig.run(args);
        assert_eq!(out.code, 0, "{}", out.err);
        assert!(!out.out.contains("tb_segredo"), "{}", out.out);
        assert!(out.out.contains("(set)"));
    }
    assert_eq!(rig.run(&["profile", "show", "nope"]).code, 2);
}

/// UC: UC-CLI-026
#[test]
fn config_show_tells_where_each_value_came_from() {
    let rig = Rig::new();
    rig.run(&[
        "profile",
        "add",
        "local",
        "--url",
        "http://a:1",
        "--timeout",
        "60",
    ]);
    rig.set_env("TABA_CLI_URL", "http://env:9");
    let out = rig.run(&["config", "show", "-o", "raw"]);
    assert_eq!(out.code, 0, "{}", out.err);
    assert!(out.out.contains("\"profile\":\"local\""), "{}", out.out);
    assert!(
        out.out.contains(
            "{\"field\":\"url\",\"value\":\"http://env:9\",\"source\":\"variable TABA_CLI_URL\"}"
        ),
        "{}",
        out.out
    );
    assert!(
        out.out
            .contains("{\"field\":\"timeout\",\"value\":\"60\",\"source\":\"profile local\"}"),
        "{}",
        out.out
    );
    assert!(
        out.out
            .contains("{\"field\":\"max-attempts\",\"value\":\"3\",\"source\":\"default\"}"),
        "{}",
        out.out
    );
    assert!(out
        .out
        .contains("{\"field\":\"token\",\"value\":null,\"source\":\"default\"}"));
    let table = rig.run(&["config", "show", "-o", "table"]);
    assert!(
        table.out.lines().next().unwrap().starts_with("field"),
        "{}",
        table.out
    );
}

/// UC: UC-CLI-027
#[test]
fn profile_session_prints_lines_and_writes_nothing() {
    let rig = Rig::new();
    rig.run(&["profile", "add", "staging", "--url", "http://h:1"]);
    let before = std::fs::read_to_string(rig.config_path()).unwrap();
    let chosen = rig.run(&["profile", "session", "staging"]);
    assert_eq!(chosen.out, "export TABA_CLI_PROFILE='staging'\n");
    let temp = rig.run(&[
        "profile",
        "session",
        "--url",
        "http://10.0.0.5:8080",
        "--token-file",
        "./tok",
    ]);
    assert_eq!(
        temp.out,
        "export TABA_CLI_URL='http://10.0.0.5:8080'\nexport TABA_CLI_TOKEN_FILE='./tok'\n"
    );
    let ps = rig.run(&[
        "profile",
        "session",
        "--url",
        "http://10.0.0.5:8080",
        "--shell",
        "powershell",
    ]);
    assert_eq!(ps.out, "$env:TABA_CLI_URL = 'http://10.0.0.5:8080'\n");
    let cmd = rig.run(&["profile", "session", "--timeout", "9", "--shell", "cmd"]);
    assert_eq!(cmd.out, "set \"TABA_CLI_TIMEOUT=9\"\n");
    let clear = rig.run(&["profile", "session", "--clear"]);
    assert!(
        clear
            .out
            .starts_with("unset TABA_CLI_PROFILE\nunset TABA_CLI_URL\n"),
        "{}",
        clear.out
    );
    assert_eq!(std::fs::read_to_string(rig.config_path()).unwrap(), before);
}

/// UC: UC-CLI-027
#[test]
fn profile_session_defaults_to_powershell_on_windows_and_checks_its_arguments() {
    let mut rig = Rig::new();
    rig.platform.os = Os::Windows;
    let out = rig.run(&["profile", "session", "--url", "http://x:1"]);
    assert_eq!(out.out, "$env:TABA_CLI_URL = 'http://x:1'\n");
    assert_eq!(rig.run(&["profile", "session"]).code, 2);
    assert_eq!(rig.run(&["profile", "session", "nope"]).code, 2);
    assert_eq!(
        rig.run(&["profile", "session", "--clear", "--url", "http://x:1"])
            .code,
        2
    );
    assert_eq!(
        rig.run(&[
            "profile",
            "session",
            "--url",
            "http://x:1",
            "--shell",
            "fish"
        ])
        .code,
        2
    );
    assert_eq!(
        rig.run(&["profile", "session", "--url", "no-scheme"]).code,
        2
    );
}

/// UC: UC-CLI-028
#[test]
fn an_invalid_config_file_exits_5_with_the_line() {
    let rig = Rig::new();
    std::fs::write(rig.config_path(), "default: a\nprofiles:\n  a: [\n").unwrap();
    for args in [
        &["topic", "list"][..],
        &["profile", "list"],
        &["profile", "add", "x"],
        &["config", "show"],
    ] {
        let out = rig.run(args);
        assert_eq!(out.code, 5, "{args:?}: {}", out.err);
        assert!(
            out.err.contains("config.yml") && out.err.contains("line 3"),
            "{}",
            out.err
        );
    }
    assert!(rig.seen().is_empty());
    assert_eq!(
        std::fs::read_to_string(rig.config_path()).unwrap(),
        "default: a\nprofiles:\n  a: [\n",
        "the invalid file is not overwritten"
    );
}

/// UC: UC-CLI-028
#[test]
fn a_hand_written_config_with_comments_and_quotes_is_read() {
    let rig = Rig::new();
    std::fs::write(
        rig.config_path(),
        "# my profiles\ndefault: stg\nprofiles:\n  stg:\n    url: \"https://taba.stg:8443\"   # staging\n    max-attempts: 5\n",
    )
    .unwrap();
    rig.mock().reply(200, &[], TOPIC);
    assert_eq!(rig.run(&["topic", "get", "x"]).code, 0);
    assert_eq!(rig.seen()[0].origin, "https://taba.stg:8443");
}

// ---------------------------------------------------------------------------------------------
// Interactive profiles screen
// ---------------------------------------------------------------------------------------------

fn with_terminal(rig: &mut Rig, keys: &[Key]) -> Arc<Mutex<Vec<Frame>>> {
    let frames = Arc::new(Mutex::new(Vec::new()));
    rig.platform.tty = true;
    *rig.platform.terminal.lock().unwrap() = Some(ScriptedTerminal {
        keys: keys.iter().copied().collect(),
        frames: frames.clone(),
    });
    frames
}

fn chars(text: &str) -> Vec<Key> {
    text.chars().map(Key::Char).collect()
}

/// UC: UC-CLI-029, UC-CLI-030
#[test]
fn the_interactive_screen_creates_a_profile_that_the_commands_then_use() {
    let mut rig = Rig::new();
    let mut keys = vec![Key::Char('n')];
    keys.extend(chars("production"));
    keys.push(Key::Down);
    keys.extend(chars("https://taba.example.com"));
    keys.push(Key::Down);
    keys.push(Key::Down);
    keys.extend(chars("/sec/prod.tok"));
    keys.extend([Key::Ctrl('s'), Key::Char('q')]);
    let frames = with_terminal(&mut rig, &keys);
    let out = rig.run(&["profile"]);
    assert_eq!(out.code, 0, "{}", out.err);
    let shown = frames
        .lock()
        .unwrap()
        .iter()
        .map(|f| f.lines.join("\n"))
        .collect::<Vec<_>>()
        .join("\n=====\n");
    assert!(
        shown.contains("taba-cli profiles")
            && shown.contains("New profile")
            && shown.contains("Profile `production` saved."),
        "{shown}"
    );

    // The file is the same one `profile add` would write, and the commands use it.
    let text = std::fs::read_to_string(rig.config_path()).unwrap();
    assert!(
        text.contains("default: production")
            && text.contains("url: https://taba.example.com")
            && text.contains("token-file: /sec/prod.tok"),
        "{text}"
    );
    let list = rig.run(&["profile", "list", "-o", "raw"]);
    assert_eq!(list.out.trim(), "{\"items\":[{\"name\":\"production\",\"default\":true,\"url\":\"https://taba.example.com\",\"token-file\":\"/sec/prod.tok\"}]}");
}

/// UC: UC-CLI-029, UC-CLI-033
#[test]
fn the_interactive_screen_removes_after_confirming_and_marks_the_default() {
    let mut rig = Rig::new();
    rig.run(&["profile", "add", "a", "--url", "http://a:1"]);
    rig.run(&["profile", "add", "b", "--url", "http://b:2"]);
    let frames = with_terminal(
        &mut rig,
        &[Key::Down, Key::Delete, Key::Char('y'), Key::Esc],
    );
    assert_eq!(rig.run(&["profile"]).code, 0);
    let first = frames.lock().unwrap()[0].lines.join("\n");
    assert!(
        first.contains("▶ a") && first.contains("●") && first.contains("○"),
        "{first}"
    );
    assert_eq!(
        rig.run(&["profile", "list", "-o", "raw"]).out.trim(),
        "{\"items\":[{\"name\":\"a\",\"default\":true,\"url\":\"http://a:1\"}]}"
    );
}

/// UC: UC-CLI-032
#[test]
fn the_interactive_test_button_calls_the_client_api_with_the_unsaved_values() {
    let mut rig = Rig::new();
    rig.mock().reply(200, &[], "{\"items\":[]}");
    let mut keys = vec![Key::Char('n')];
    keys.extend(chars("t"));
    keys.push(Key::Down);
    keys.extend(chars("http://test:7"));
    keys.push(Key::Down);
    keys.push(Key::Down);
    keys.push(Key::Down);
    keys.extend(chars("tb_visivel"));
    keys.extend([Key::Down; 8]);
    keys.push(Key::Enter);
    keys.extend([Key::Esc, Key::Esc, Key::Char('q')]);
    let frames = with_terminal(&mut rig, &keys);
    assert_eq!(rig.run(&["profile"]).code, 0);
    let seen = rig.seen();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].origin, "http://test:7");
    assert_eq!(seen[0].request.target, "/topics?limit=1");
    assert_eq!(seen[0].header("Authorization"), Some("Bearer tb_visivel"));
    let shown = frames
        .lock()
        .unwrap()
        .iter()
        .map(|f| f.lines.join("\n"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(shown.contains("ok: http://test:7 answered in"), "{shown}");
    assert!(!rig.config_path().exists(), "testing does not save");
}

/// UC: UC-CLI-036
#[test]
fn outside_a_terminal_the_profile_command_prints_help_and_exits_2() {
    let rig = Rig::new();
    let out = rig.run(&["profile"]);
    assert_eq!(out.code, 2);
    assert!(
        out.err.contains("needs a terminal")
            && out.err.contains("profile add")
            && out.err.contains("profile session"),
        "{}",
        out.err
    );
    assert_eq!(out.out, "");

    // With the input redirected but no real terminal, it does not open either.
    let mut rig = Rig::new();
    rig.platform.tty = true;
    let out = rig.run(&["profile"]);
    assert_eq!(
        out.code, 2,
        "without an available terminal the screen does not open"
    );
}

struct ClosedPipe;

impl io::Write for ClosedPipe {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::BrokenPipe.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// UC: UC-CLI-004
#[test]
fn a_closed_output_pipe_ends_quietly_like_piping_into_head() {
    let rig = Rig::new();
    rig.mock().reply(200, &[], TOPIC);
    let args: Vec<String> = ["topic", "get", "x"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let mut err = Vec::new();
    let mut input = Cursor::new(Vec::new());
    let code = run(&rig.platform, &args, &mut ClosedPipe, &mut err, &mut input);
    assert_eq!(code, 0);
    assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
}

// ---------------------------------------------------------------------------------------------
// The full-screen interface (doc 34): what can be checked without a real terminal
// ---------------------------------------------------------------------------------------------

/// TUI: TUI-FRM-06
#[cfg(feature = "tui")]
#[test]
fn outside_a_terminal_the_tui_command_prints_help_and_exits_2_without_touching_the_network() {
    let rig = Rig::new();
    let out = rig.run(&["tui"]);
    assert_eq!(out.code, 2);
    assert!(
        out.err.contains("needs a terminal") && out.err.contains("taba-cli tui"),
        "{}",
        out.err
    );
    assert_eq!(out.out, "");
    assert!(rig.seen().is_empty());
    assert!(!rig.config_path().exists());
}

/// TUI: TUI-HLP-03
#[cfg(feature = "tui")]
#[test]
fn the_tui_refuses_a_language_it_does_not_have_before_opening_anything() {
    let mut rig = Rig::new();
    rig.platform.tty = true;
    let out = rig.run(&["tui", "--lang", "fr"]);
    assert_eq!(out.code, 2);
    assert!(
        out.err.contains("--lang") && out.err.contains("en-US or pt-BR"),
        "{}",
        out.err
    );
    assert!(rig.seen().is_empty());
}

/// TUI: TUI-MON-05
#[cfg(feature = "tui")]
#[test]
fn the_tui_refuses_a_history_that_is_too_long_or_too_dense() {
    let mut rig = Rig::new();
    rig.platform.tty = true;
    let out = rig.run(&["tui", "--history-retention", "7h"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("6h or less"), "{}", out.err);
    let out = rig.run(&["tui", "--history-interval", "0s"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("1s or more"), "{}", out.err);
    let out = rig.run(&["tui", "--history-retention", "soon"]);
    assert_eq!(out.code, 2);
    assert!(out.err.contains("is not a time"), "{}", out.err);
}

/// TUI: TUI-HLP-03
#[test]
fn help_lists_the_tui_command_and_the_lang_option() {
    let rig = Rig::new();
    let all = rig.run(&["help"]);
    assert!(
        all.out.contains("tui") && all.out.contains("--lang"),
        "{}",
        all.out
    );
    let one = rig.run(&["help", "tui"]);
    assert!(one.out.contains("usage: taba-cli tui"), "{}", one.out);
    assert!(
        one.out.contains("--no-mouse") && one.out.contains("--history-retention"),
        "{}",
        one.out
    );
}

/// TUI: TUI-HLP-03
#[test]
fn the_language_and_history_settings_live_in_the_config_file_beside_the_profiles() {
    let rig = Rig::new();
    std::fs::write(
        rig.config_path(),
        "default: prod\nui:\n  language: pt-BR\nhistory:\n  retention: 30m\n  interval: 10s\nprofiles:\n  prod:\n    url: http://t:1\n",
    )
    .unwrap();
    rig.mock().reply(200, &[], TOPIC);
    // The profile commands keep working with the new keys in the file, and do not lose them on a rewrite.
    assert_eq!(rig.run(&["topic", "get", "orders"]).code, 0);
    assert_eq!(
        rig.run(&["profile", "set", "prod", "--timeout", "9"]).code,
        0
    );
    let text = std::fs::read_to_string(rig.config_path()).unwrap();
    assert!(text.contains("language: pt-BR"), "{text}");
    assert!(
        text.contains("retention: 30m") && text.contains("interval: 10s"),
        "{text}"
    );
    assert!(text.contains("timeout: 9"), "{text}");
}
