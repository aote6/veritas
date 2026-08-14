//! Forge-like JSONL 端到端：启动 veritasd，驱动完整 JSON-Lines 协议，验证全链路。
//!
//! 验证内容：真实进程间协议下的对象创建、授权、link、commit 等核心路径。
//! 对应 VERIFICATION_MAP：forge_e2e_jsonlines.rs
//! 若失败，意味着外部 JSONL 接口与 Kernel 行为不一致或 e2e 链路断裂。

use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

/// Full Forge-like E2E: start veritasd, drive JSON-Lines, verify the whole chain.
#[test]
fn forge_e2e_create_write_read_commit_observe() {
    let veritasd_path = std::path::PathBuf::from(env!("CARGO_BIN_EXE_veritasd"));

    // Use a temp WAL so the test is isolated
    let wal_path = format!(
        "{}/veritas_forge_e2e_{}.wal",
        std::env::temp_dir().display(),
        std::process::id()
    );
    let _ = std::fs::remove_file(&wal_path);

    let mut child = Command::new(&veritasd_path)
        .env("VERITAS_WAL", &wal_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("failed to start veritasd");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);

    let mut send = |json: &str| {
        writeln!(stdin, "{}", json).unwrap();
        stdin.flush().unwrap();
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str::<serde_json::Value>(line.trim()).unwrap()
    };

    // 1. ping
    let r = send(r#"{"cmd":"ping"}"#);
    assert_eq!(r["ok"], true);
    assert_eq!(r["result"], "pong");

    // 2. attach_identity
    let r = send(r#"{"cmd":"attach_identity"}"#);
    assert_eq!(r["ok"], true);
    let identity_id = r["object_id"].as_u64().unwrap();
    assert!(identity_id > 0);

    // 3. whoami
    let r = send(r#"{"cmd":"whoami"}"#);
    assert_eq!(r["ok"], true);
    assert_eq!(r["object_id"].as_u64().unwrap(), identity_id);

    // 4. tx_begin
    let r = send(r#"{"cmd":"tx_begin"}"#);
    assert_eq!(r["ok"], true);
    let sid = r["session_id"].as_u64().unwrap();
    assert!(sid > 0);

    // 5. tx_create_object
    let cmd = format!(r#"{{"cmd":"tx_create_object","session_id":{}}}"#, sid);
    let r = send(&cmd);
    assert_eq!(r["ok"], true);
    let obj_id = r["object_id"].as_u64().unwrap();
    assert!(obj_id > 0);

    // 6. tx_write state_id=0 (path)
    let cmd = format!(
        r#"{{"cmd":"tx_write","session_id":{},"state_id":0,"value":"/hello.txt"}}"#,
        sid
    );
    let r = send(&cmd);
    assert_eq!(r["ok"], true);

    // 7. tx_write state_id=1 (content)
    let cmd = format!(
        r#"{{"cmd":"tx_write","session_id":{},"state_id":1,"value":"hello world from forge"}}"#,
        sid
    );
    let r = send(&cmd);
    assert_eq!(r["ok"], true);

    // 8. tx_read state_id=1 — verify pending write is visible within tx
    let cmd = format!(r#"{{"cmd":"tx_read","session_id":{},"state_id":1}}"#, sid);
    let r = send(&cmd);
    assert_eq!(r["ok"], true);
    assert_eq!(r["object_id"], serde_json::Value::Null); // tx_read doesn't echo object_id
    let hex_str = r["value_hex"].as_str().unwrap();
    let decoded = hex::decode(hex_str).unwrap();
    assert_eq!(
        String::from_utf8(decoded).unwrap(),
        "hello world from forge"
    );

    // 9. tx_commit
    let cmd = format!(r#"{{"cmd":"tx_commit","session_id":{}}}"#, sid);
    let r = send(&cmd);
    assert_eq!(r["ok"], true);
    let receipt = &r["receipt"];
    assert!(receipt["before_root"].as_u64().unwrap() > 0);
    assert!(receipt["after_root"].as_u64().unwrap() > 0);
    assert_ne!(receipt["before_root"], receipt["after_root"]);
    assert_eq!(
        receipt["delta"]["objects_created"][0].as_u64().unwrap(),
        obj_id
    );
    assert_eq!(
        receipt["delta"]["memory_written"].as_array().unwrap().len(),
        2
    );

    // 10. world_info after commit
    let r = send(r#"{"cmd":"world_info"}"#);
    assert_eq!(r["ok"], true);
    assert_eq!(
        r["version"].as_u64().unwrap(),
        receipt["version"].as_u64().unwrap()
    );
    assert_eq!(
        r["state_root"].as_u64().unwrap(),
        receipt["after_root"].as_u64().unwrap()
    );
    assert!(r["object_count"].as_u64().unwrap() >= 2);

    // 11. new tx_begin + tx_read to confirm committed state is observable
    let r = send(r#"{"cmd":"tx_begin"}"#);
    assert_eq!(r["ok"], true);
    let sid2 = r["session_id"].as_u64().unwrap();

    let cmd = format!(r#"{{"cmd":"tx_read","session_id":{},"state_id":1}}"#, sid2);
    let r = send(&cmd);
    assert_eq!(r["ok"], true);
    let hex_str2 = r["value_hex"].as_str().unwrap();
    let decoded2 = hex::decode(hex_str2).unwrap();
    assert_eq!(
        String::from_utf8(decoded2).unwrap(),
        "hello world from forge"
    );

    // Abort the read-only tx
    let cmd = format!(r#"{{"cmd":"tx_abort","session_id":{}}}"#, sid2);
    let r = send(&cmd);
    assert_eq!(r["ok"], true);

    // Cleanup
    drop(stdin);
    let _ = child.wait();
    let _ = std::fs::remove_file(&wal_path);
}
