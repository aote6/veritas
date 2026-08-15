//! Meta-test: 强制所有测试文件有文档注释。
//!
//! 规则:
//! 1. 每个 tests/*.rs 文件必须有 //! 文件级注释说明验证什么
//! 2. 每个 #[test] 函数上方 3 行内必须有 /// 注释
//!
//! 为什么需要:
//! - 没有注释的测试 = 没人知道它验证什么
//! - 会导致未来会话反复质疑已验证的结论
//! - gen_verification_map.py 依赖 //! 注释生成验证地图

use std::fs;
use std::path::Path;

/// @category: D
/// @layer: integration
/// @testworld: NOT_USED
/// @req: INT-04
#[test]
fn all_test_files_have_doc_comments() {
    let tests_dir = Path::new("tests");
    let mut failures = Vec::new();

    for entry in fs::read_dir(tests_dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }

        let content = fs::read_to_string(&path).unwrap();
        let fname = path.file_name().unwrap().to_string_lossy().to_string();

        if fname == "meta_verification_comments.rs" {
            continue;
        }

        if !content.contains("//!") {
            failures.push(format!("{}: missing //! file-level doc comment", fname));
        }

        let lines: Vec<&str> = content.lines().collect();
        for i in 0..lines.len() {
            if lines[i].contains("#[test]") {
                let mut has_doc = false;
                for j in (0..i).rev().take(3) {
                    if lines[j].contains("///") {
                        has_doc = true;
                        break;
                    }
                }
                if !has_doc {
                    failures.push(format!(
                        "{}:{} #[test] missing /// doc comment",
                        fname,
                        i + 1
                    ));
                }
            }
        }
    }

    assert!(
        failures.is_empty(),
        "以下测试缺少文档注释 ({} 个):\n{}",
        failures.len(),
        failures.join("\n")
    );
}
