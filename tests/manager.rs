use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use depends_on_rs::Manager;

fn temp_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time should be monotonic")
        .as_nanos();
    let path = std::env::temp_dir().join(format!("depends-on-rs-{prefix}-{nanos}"));
    fs::create_dir_all(&path).expect("temp dir should create");
    path
}

#[test]
fn parses_and_rejects_cycles() {
    let err = Manager::parse(
        br#"{
          "a": {"cmd": ["sh", "-c", "echo a"], "depends": ["b"]},
          "b": {"cmd": ["sh", "-c", "echo b"], "depends": ["a"]}
        }"#,
    )
    .expect_err("cycle should fail");

    assert!(err.to_string().contains("cycle"));
}

#[test]
fn start_waits_for_log_pattern() {
    let cfg = br#"{
      "server": {
        "cmd": ["sh", "-c", "printf 'booting\n'; printf 'ready now\n'; sleep 5"],
        "wait_for": {"log_pattern": "ready now", "timeout": "2s"},
        "fds": {"stdout": "null", "stderr": "null"}
      }
    }"#;

    let manager = Manager::parse(cfg).expect("config should parse");
    let handle = manager
        .start(&["server".to_string()])
        .expect("target should reach readiness");
    drop(handle);
}

#[test]
fn start_waits_for_exit_code() {
    let cfg = br#"{
      "prep": {
        "cmd": ["sh", "-c", "exit 0"],
        "wait_for": {"exit_code": 0, "timeout": "2s"}
      }
    }"#;

    let manager = Manager::parse(cfg).expect("config should parse");
    let handle = manager
        .start(&["prep".to_string()])
        .expect("target should exit cleanly");
    drop(handle);
}

#[test]
fn start_waits_for_port() {
    let cfg = br#"{
      "server": {
        "cmd": ["python3", "-c", "import http.server, socketserver; socketserver.TCPServer(('127.0.0.1', 18081), http.server.SimpleHTTPRequestHandler).serve_forever()"],
        "wait_for": {"port": 18081, "timeout": "5s"},
        "fds": {"stdout": "null", "stderr": "null"}
      }
    }"#;

    let manager = Manager::parse(cfg).expect("config should parse");
    let handle = manager
        .start(&["server".to_string()])
        .expect("port should become reachable");
    drop(handle);
}

#[test]
fn run_command_starts_dependencies_and_runs_follow_up_command() {
    let dir = temp_dir("run-command");
    let prep_path = dir.join("prep.txt");
    let output_path = dir.join("out.txt");
    let cfg = format!(
        r#"{{
          "prep": {{
            "cmd": ["sh", "-c", "printf ready > {prep}"],
            "wait_for": {{"exit_code": 0, "timeout": "2s"}}
          }}
        }}"#,
        prep = prep_path.display()
    );

    let manager = Manager::parse(cfg.as_bytes()).expect("config should parse");
    let status = manager
        .run_command(
            &["prep".to_string()],
            &[
                "sh".to_string(),
                "-c".to_string(),
                format!("cat {} > {}", prep_path.display(), output_path.display()),
            ],
        )
        .expect("run_command should succeed");

    assert_eq!(status, 0);
    assert_eq!(
        fs::read_to_string(output_path).expect("output should exist"),
        "ready"
    );
}
