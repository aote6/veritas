// P1 JSONL e2e: 验证真实的 JSON request -> veritasd -> WorldApi -> Kernel -> Engine
// -> CapabilityGraph 全链路，而不仅是 Rust 内部函数调用。
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};

#[test]
fn tx_capability_grant_jsonl_end_to_end() {
    let veritasd_path = std::path::PathBuf::from(env!("CARGO_BIN_EXE_veritasd"));
    let wal_path = format!(
        "{}/veritas_capgrant_p1_e2e_{}.wal",
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

    // A attaches identity.
    let r = send(r#"{"cmd":"attach_identity"}"#);
    assert_eq!(r["ok"], true);
    let a = r["object_id"].as_u64().unwrap();

    // A creates B and C in one session, commits.
    let r = send(r#"{"cmd":"tx_begin"}"#);
    assert_eq!(r["ok"], true);
    let sid0 = r["session_id"].as_u64().unwrap();

    let r = send(&format!(r#"{{"cmd":"tx_create_object","session_id":{}}}"#, sid0));
    assert_eq!(r["ok"], true);
    let b = r["object_id"].as_u64().unwrap();

    let r = send(&format!(r#"{{"cmd":"tx_create_object","session_id":{}}}"#, sid0));
    assert_eq!(r["ok"], true);
    let c = r["object_id"].as_u64().unwrap();

    let r = send(&format!(r#"{{"cmd":"tx_commit","session_id":{}}}"#, sid0));
    assert_eq!(r["ok"], true);

    // Unauthorized: separate identity/session for B attempts link to C, must fail at commit.
    let r = send(&format!(r#"{{"cmd":"tx_begin","actor_id":{}}}"#, b));
    assert_eq!(r["ok"], true);
    let sid_bad = r["session_id"].as_u64().unwrap();
    let r = send(&format!(
        r#"{{"cmd":"tx_link","session_id":{},"from":{},"to":{},"link_type":"owns"}}"#,
        sid_bad, b, c
    ));
    assert_eq!(r["ok"], true, "tx_link only stages; commit enforces authorization");
    let r = send(&format!(r#"{{"cmd":"tx_commit","session_id":{}}}"#, sid_bad));
    assert_eq!(r["ok"], false, "未授权的 B 对 C 的 link 必须在 commit 时被拒绝");

    // A grants B a link capability on C via the new external primitive.
    let r = send(&format!(r#"{{"cmd":"tx_begin","actor_id":{}}}"#, a));
    assert_eq!(r["ok"], true);
    let sid1 = r["session_id"].as_u64().unwrap();
    let r = send(&format!(
        r#"{{"cmd":"tx_capability_grant","session_id":{},"grantor":{},"grantee":{},"capability_type":"link","resource":{}}}"#,
        sid1, a, b, c
    ));
    assert_eq!(r["ok"], true, "tx_capability_grant 应当成功");
    let r = send(&format!(r#"{{"cmd":"tx_commit","session_id":{}}}"#, sid1));
    assert_eq!(r["ok"], true);

    // Authorized: B's new session can now link to C.
    let r = send(&format!(r#"{{"cmd":"tx_begin","actor_id":{}}}"#, b));
    assert_eq!(r["ok"], true);
    let sid2 = r["session_id"].as_u64().unwrap();
    let r = send(&format!(
        r#"{{"cmd":"tx_link","session_id":{},"from":{},"to":{},"link_type":"owns"}}"#,
        sid2, b, c
    ));
    assert_eq!(r["ok"], true);
    let r = send(&format!(r#"{{"cmd":"tx_commit","session_id":{}}}"#, sid2));
    assert_eq!(r["ok"], true, "B 持有 A 授予的 capability 后，对 C 的 link 应当在 commit 时成功");

    // Cleanup
    drop(stdin);
    let _ = child.wait();
    let _ = std::fs::remove_file(&wal_path);
}
