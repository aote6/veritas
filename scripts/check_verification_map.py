#!/usr/bin/env python3
import sys
import re
from pathlib import Path

# ==================================================
# 1. 核心策略与契约 (Contract & Policies)
# ==================================================

VALID_CATEGORIES = {'A', 'B', 'C', 'D'}
VALID_LAYERS = {'kernel', 'capability', 'transaction', 'recovery', 'integration'}
VALID_TESTWORLDS = {'FORBIDDEN', 'NOT_USED', 'ALLOWED', 'PREFERRED'}

# Category 对应的 TestWorld 使用策略
CATEGORY_TESTWORLD_POLICY = {
    'A': {'FORBIDDEN'},
    'B': {'FORBIDDEN'},
    'C': {'FORBIDDEN'},
    'D': {'NOT_USED', 'ALLOWED', 'PREFERRED'}
}

# ==================================================
# 2. Rust 解析引擎 (Robust Rust Parser)
# ==================================================

# 匹配 Rust raw string: r"...", r#"..."#, r##"..."##, ...
# 使用反向引用 \1 强制开闭 # 数量一致；跨行安全 (re.DOTALL)。
# \b 防止把标识符尾部的 'r' (如 "xr\"...\"" 的 r) 误判为 raw string 前缀。
_RAW_STRING_RE = re.compile(r'\br(#*)"(.*?)"\1', re.DOTALL)


def _preserve_newlines(matched_text):
    """用等量换行符替换匹配文本，保持清洗前后的行号严格对齐。"""
    return '\n' * matched_text.count('\n')


def clean_for_analysis(code_str):
    """
    去除代码中的注释和字符串，防止大括号匹配错位或 TestWorld 在注释/字符串中被误报。

    在整段文本（而非逐行）上操作，因此可以正确处理跨行的 raw string
    (r"...", r#"..."#, r##"..."##, 以及多行 raw string)。所有替换都保留
    原始换行符数量，确保清洗后的文本按 '\n' split 得到的行号与原文件行号
    一一对应。
    """
    # 1) 先处理 raw string，避免其内部的 // 或 /* */ 被后续步骤误当作注释
    code_str = _RAW_STRING_RE.sub(lambda m: _preserve_newlines(m.group(0)), code_str)
    # 2) 移除块级注释（可能跨行）
    code_str = re.sub(
        r'/\*.*?\*/',
        lambda m: _preserve_newlines(m.group(0)),
        code_str,
        flags=re.DOTALL,
    )
    # 3) 移除行内注释（不含换行，安全）
    code_str = re.sub(r'//[^\n]*', '', code_str)
    # 4) 移除标准字符串（含 Rust 的 \<newline> 续行，以及字面量跨行字符串；
    #    DOTALL 让 \\. 里的 "." 能匹配到续行的换行符本身，否则匹配会在
    #    反斜杠处失败而错误地把闭合引号让位给后面不相关代码里的下一个
    #    引号，导致大段代码被误判为字符串内容）
    code_str = re.sub(
        r'"(?:\\.|[^"\\])*"',
        lambda m: '"' + _preserve_newlines(m.group(0)) + '"',
        code_str,
        flags=re.DOTALL,
    )
    return code_str


def extract_source_tests(tests_dir):
    """
    提取 tests/**/*.rs 中的所有 #[test] 及其 Metadata 和函数体。
    """
    tests = {}
    duplicates = []

    for file_path in Path(tests_dir).rglob('*.rs'):
        if not file_path.is_file():
            continue

        content = file_path.read_text(encoding='utf-8')
        lines = content.split('\n')

        # 对整个文件做一次清洗，得到与 lines 行号严格对齐的 cleaned_lines。
        # 后续的 #[test] 识别、fn 声明查找、大括号计数都基于 cleaned_lines，
        # 这样注释/字符串/raw string 中的干扰内容不会影响判定。
        cleaned_content = clean_for_analysis(content)
        cleaned_lines = cleaned_content.split('\n')
        if len(cleaned_lines) != len(lines):
            # 防御性兜底：理论上不应发生（所有替换都保留换行数），
            # 但如果发生，裁剪/补齐以避免索引越界，而不是崩溃。
            if len(cleaned_lines) < len(lines):
                cleaned_lines += [''] * (len(lines) - len(cleaned_lines))
            else:
                cleaned_lines = cleaned_lines[:len(lines)]

        for i, line in enumerate(lines):
            cl_line = cleaned_lines[i]
            if '#[test]' in cl_line or '#[tokio::test]' in cl_line:
                # 寻找函数声明（在清洗后的文本里找，避免注释里的假 fn 声明）
                fn_name = None
                j = i + 1
                while j < len(lines) and j <= i + 5:
                    cl_str = cleaned_lines[j].strip()
                    if cl_str == '':
                        j += 1
                        continue
                    # 匹配普通函数或 async 函数
                    m = re.match(r'^(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\(', cl_str)
                    if m:
                        fn_name = m.group(1)
                        break
                    j += 1

                if not fn_name:
                    continue

                test_id = f"{file_path}::{fn_name}"
                if test_id in tests:
                    duplicates.append(test_id)

                # 向上寻找 Metadata (/// @key: value)
                meta = {'req': []}
                idx = i - 1
                while idx >= 0:
                    ml = lines[idx].strip()
                    if not ml.startswith('///'):
                        if ml == '' or ml.startswith('//'):
                            idx -= 1
                            continue
                        break # 中断，metadata 必须紧贴 test 宏

                    match = re.match(r'^///\s*@([a-z]+):\s*(.+)$', ml)
                    if match:
                        k, v = match.group(1), match.group(2).strip()
                        if k == 'req':
                            meta['req'].insert(0, v) # 保持多个 req 的声明顺序
                        else:
                            meta[k] = v
                    idx -= 1

                # 提取函数体（免疫嵌套大括号 / 字符串 / raw string 干扰）
                body_lines = []
                brace_count = 0
                started = False
                k = j
                while k < len(lines):
                    cl = cleaned_lines[k]
                    if not started:
                        if '{' in cl:
                            started = True
                            idx_brace = cl.find('{')
                            brace_count += cl[idx_brace:].count('{') - cl[idx_brace:].count('}')
                            body_lines.append(lines[k])
                    else:
                        brace_count += cl.count('{') - cl.count('}')
                        body_lines.append(lines[k])

                    if started and brace_count <= 0:
                        break
                    k += 1

                tests[test_id] = {
                    'meta': meta if any(meta.values()) else {},
                    'body': '\n'.join(body_lines),
                    'file': str(file_path)
                }
    return tests, duplicates


def parse_verification_map(map_path):
    """
    解析 VERIFICATION_MAP.md 表格内容。
    """
    map_tests = {}
    if not Path(map_path).exists():
        return map_tests

    content = Path(map_path).read_text(encoding='utf-8')
    for line in content.split('\n'):
        line = line.strip()
        # 跳过表头和分隔符
        if not line.startswith('|') or '---' in line or 'Test ID' in line:
            continue

        parts = [p.strip() for p in line.split('|')]
        # format: | tid | category | layer | testworld | req |
        if len(parts) >= 2:
            tid = parts[1]
            if '.rs::' in tid:
                meta = {}
                if len(parts) >= 6:
                    c = parts[2]
                    l = parts[3]
                    tw = parts[4]
                    req_raw = parts[5]

                    if c or l or tw or req_raw:
                        meta['category'] = c
                        meta['layer'] = l
                        meta['testworld'] = tw
                        meta['req'] = [r.strip() for r in req_raw.split(',') if r.strip()]
                map_tests[tid] = meta
    return map_tests


# ==================================================
# 2b. Self-Test (轻量自检，不依赖 pytest/syn/tree-sitter)
# ==================================================

def _fn_names_in_file(tests, filename_suffix):
    return {tid.split('::')[-1] for tid in tests if filename_suffix in tid}


def _find_id(tests, filename_suffix, fn_name):
    for tid in tests:
        if filename_suffix in tid and tid.endswith(f"::{fn_name}"):
            return tid
    return None


def _body_extracted_fully(tests, filename_suffix, fn_name):
    """粗略校验函数体是否被完整提取（未因大括号误计数而提前截断）。"""
    tid = _find_id(tests, filename_suffix, fn_name)
    if not tid:
        return False
    body = tests[tid]['body']
    return 'assert' in body and body.rstrip().endswith('}')


def self_test():
    import tempfile
    import shutil
    import textwrap

    failures = []

    def check(name, cond):
        if cond:
            print(f"  [PASS] {name}")
        else:
            failures.append(name)
            print(f"  [FAIL] {name}")

    tmpdir = tempfile.mkdtemp(prefix="veritas_selftest_")
    try:
        tests_dir = Path(tmpdir) / "tests"
        tests_dir.mkdir()

        # Case 1: 注释/字符串中的 #[test] 不应被识别
        (tests_dir / "case1_comment_string.rs").write_text(textwrap.dedent('''
            // This is not a real test: #[test]
            /* also not real: #[test] fn fake() {} */
            fn helper() {
                let s = "contains #[test] inside a string";
            }

            #[test]
            fn real_test_one() {
                assert!(true);
            }
        '''))

        # Case 2: #[test] + #[should_panic] 应正确识别
        (tests_dir / "case2_should_panic.rs").write_text(textwrap.dedent('''
            #[test]
            #[should_panic]
            fn real_test_panics() {
                panic!("boom");
            }
        '''))

        # Case 3: 普通字符串中的 {} 不应影响 brace extraction
        (tests_dir / "case3_string_braces.rs").write_text(textwrap.dedent('''
            #[test]
            fn real_test_braces_in_string() {
                let s = "{ this looks like a brace but is just text } {{{";
                assert_eq!(s.len() > 0, true);
            }
        '''))

        # Case 4/5: r"...", r#"..."#, r##"..."## 以及多行 raw string 中的 {}
        # 不应影响 extraction
        (tests_dir / "case4_raw_strings.rs").write_text(textwrap.dedent('''
            #[test]
            fn real_test_raw_strings() {
                let a = r"{ raw no hash }";
                let b = r#"{ raw one hash "quoted" }"#;
                let c = r##"{ raw two hash "# still inside }"##;
                let d = r#"
                    multi
                    line { raw
                    string } spanning
                    several lines
                "#;
                assert!(a.len() + b.len() + c.len() + d.len() > 0);
            }
        '''))

        # Case 6: 字符串/注释中出现的 TestWorld 不应触发 FORBIDDEN 检查
        (tests_dir / "case6_testworld_in_string.rs").write_text(textwrap.dedent('''
            /// @category: A
            /// @layer: kernel
            /// @testworld: FORBIDDEN
            /// @req: KERNEL-001
            #[test]
            fn real_test_testworld_in_string_only() {
                let msg = "do not use TestWorld here";
                // comment mentioning TestWorld too
                assert!(msg.contains("TestWorld"));
            }
        '''))

        # Case 7: 真正代码中的 TestWorld 应能被识别
        (tests_dir / "case7_testworld_real.rs").write_text(textwrap.dedent('''
            #[test]
            fn real_test_testworld_real_usage() {
                let tw = TestWorld::new();
                assert!(tw.is_ready());
            }
        '''))

        # Case 8: 基本 metadata 能正确解析
        (tests_dir / "case8_metadata.rs").write_text(textwrap.dedent('''
            /// @category: B
            /// @layer: capability
            /// @testworld: NOT_USED
            /// @req: CAP-010
            /// @req: CAP-011
            #[test]
            fn real_test_metadata_basic() {
                assert!(true);
            }
        '''))

        source_tests, duplicates = extract_source_tests(str(tests_dir))

        print("SELF-TEST: check_verification_map.py\n")

        check(
            "case1: fake #[test] in comment/string not counted",
            _fn_names_in_file(source_tests, 'case1_comment_string.rs') == {'real_test_one'}
        )

        check(
            "case2: #[test] + #[should_panic] recognized",
            _find_id(source_tests, 'case2_should_panic.rs', 'real_test_panics') is not None
        )

        check(
            "case3: braces inside normal string don't break extraction",
            _body_extracted_fully(source_tests, 'case3_string_braces.rs', 'real_test_braces_in_string')
        )

        check(
            "case4/5: raw strings (r, r#, r##, multiline) don't break extraction",
            _body_extracted_fully(source_tests, 'case4_raw_strings.rs', 'real_test_raw_strings')
        )

        tid6 = _find_id(source_tests, 'case6_testworld_in_string.rs', 'real_test_testworld_in_string_only')
        clean_body6 = clean_for_analysis(source_tests[tid6]['body']) if tid6 else ""
        check(
            "case6: TestWorld only inside string/comment not flagged as usage",
            tid6 is not None and re.search(r"\bTestWorld\b", clean_body6) is None
        )

        tid7 = _find_id(source_tests, 'case7_testworld_real.rs', 'real_test_testworld_real_usage')
        clean_body7 = clean_for_analysis(source_tests[tid7]['body']) if tid7 else ""
        check(
            "case7: real TestWorld usage in code is detected",
            tid7 is not None and re.search(r"\bTestWorld\b", clean_body7) is not None
        )

        tid8 = _find_id(source_tests, 'case8_metadata.rs', 'real_test_metadata_basic')
        meta8 = source_tests[tid8]['meta'] if tid8 else {}
        check(
            "case8: basic metadata parsed correctly",
            tid8 is not None
            and meta8.get('category') == 'B'
            and meta8.get('layer') == 'capability'
            and meta8.get('testworld') == 'NOT_USED'
            and meta8.get('req') == ['CAP-010', 'CAP-011']
        )

        check("no false duplicate test IDs introduced", len(duplicates) == 0)

    finally:
        shutil.rmtree(tmpdir, ignore_errors=True)

    print()
    if failures:
        print(f"SELF-TEST STATUS: FAIL ({len(failures)} failing)")
        for f in failures:
            print(f"  - {f}")
        return 1
    else:
        print("SELF-TEST STATUS: PASS")
        return 0


# ==================================================
# 3. 核心检验流 (Execution & Validation)
# ==================================================

def main():
    valid_modes = ['--phase1', '--phase2', '--phase2-strict', '--self-test']
    if len(sys.argv) < 2 or sys.argv[1] not in valid_modes:
        print(f"Usage: python3 scripts/check_verification_map.py [{' | '.join(valid_modes)}]")
        sys.exit(2)

    mode = sys.argv[1]

    if mode == '--self-test':
        sys.exit(self_test())

    is_strict = (mode == '--phase2-strict')

    print("VERITAS VERIFICATION MAP AUDIT")
    print("================================\n")

    source_tests, duplicates = extract_source_tests('tests')
    map_tests = parse_verification_map('docs/VERIFICATION_MAP.md')

    violations = []

    # --------------------------------------------------
    # PHASE 1: ID Parity Validation
    # --------------------------------------------------
    if duplicates:
        for d in set(duplicates):
            violations.append(f"CHECK-02: Duplicate Test ID found in source: {d}")

    source_ids = set(source_tests.keys())
    map_ids = set(map_tests.keys())

    missing_in_map = source_ids - map_ids
    missing_in_src = map_ids - source_ids

    if mode == '--phase1':
        print("PHASE 1: TEST ID PARITY\n")
        print(f"SOURCE TESTS : {len(source_ids)}")
        print(f"MAP TESTS    : {len(map_ids)}")
        print(f"MISSING      : {len(missing_in_map)}")
        print(f"EXTRA        : {len(missing_in_src)}\n")

        if missing_in_map or missing_in_src or duplicates:
            print("STATUS: FAIL\n\nViolations:")
            if missing_in_map:
                violations.append(f"CHECK-01: Source has {len(missing_in_map)} tests not in Map (e.g., {list(missing_in_map)[0]})")
            if missing_in_src:
                violations.append(f"CHECK-01: Map has {len(missing_in_src)} tests not in Source (e.g., {list(missing_in_src)[0]})")
            for v in violations[:20]: print(f"- {v}")
            sys.exit(1)
        else:
            print("STATUS: PASS")
            sys.exit(0)

    # --------------------------------------------------
    # PHASE 2: Metadata Validation & Contamination Check
    # --------------------------------------------------
    if missing_in_map or missing_in_src:
        violations.append("CHECK-01: ID parity failure. Run --phase1 to resolve sync issues before checking metadata.")

    tagged_count = 0
    testworld_violations = 0

    for tid, tdata in source_tests.items():
        smeta = tdata['meta']
        is_tagged = bool(smeta)

        if is_tagged:
            tagged_count += 1

            # CHECK-03: Missing metadata fields
            if 'category' not in smeta or 'layer' not in smeta or 'testworld' not in smeta or not smeta.get('req'):
                violations.append(f"CHECK-03: {tid} is missing required metadata fields. Found: {smeta}")

            # CHECK-04: Invalid category
            cat = smeta.get('category')
            if cat and cat not in VALID_CATEGORIES:
                violations.append(f"CHECK-04: {tid} has invalid category '{cat}'")

            # CHECK-05: Invalid layer
            lay = smeta.get('layer')
            if lay and lay not in VALID_LAYERS:
                violations.append(f"CHECK-05: {tid} has invalid layer '{lay}'")

            # CHECK-06: Invalid TestWorld policy
            tw = smeta.get('testworld')
            if tw and tw not in VALID_TESTWORLDS:
                violations.append(f"CHECK-06: {tid} has invalid TestWorld policy '{tw}'")

            # CHECK-07: Category/TestWorld mismatch
            if cat in CATEGORY_TESTWORLD_POLICY and tw:
                if tw not in CATEGORY_TESTWORLD_POLICY[cat]:
                    violations.append(f"CHECK-07: {tid} Category '{cat}' conflicts with TestWorld '{tw}'")

            # CHECK-08: Forbidden TestWorld usage (Contamination Detection)
            if tw == 'FORBIDDEN':
                clean_body = clean_for_analysis(tdata['body'])
                if re.search(r"\bTestWorld\b", clean_body):
                    violations.append(f"CHECK-08: {tid} is FORBIDDEN to use TestWorld, but primitive was detected in body!")
                    testworld_violations += 1

            # CHECK-09: Invalid requirement format
            for req in smeta.get('req', []):
                if not re.match(r'^[A-Z]+-\d+$', req):
                    violations.append(f"CHECK-09: {tid} invalid requirement format '{req}'")

        elif is_strict:
            violations.append(f"CHECK-03: {tid} has no metadata (Strict Mode enforces 100% tagging)")

        # CHECK-10: Source vs Map metadata mismatch
        if tid in map_tests:
            mmeta = map_tests[tid]
            if smeta and not mmeta:
                violations.append(f"CHECK-10: {tid} metadata missing in MAP.")
            elif smeta and mmeta:
                for k in ['category', 'layer', 'testworld']:
                    sv = smeta.get(k, '')
                    mv = mmeta.get(k, '')
                    if sv != mv:
                        violations.append(f"CHECK-10: {tid}\n  [{k} mismatch] source={sv} | map={mv}")

                sreq = sorted(smeta.get('req', []))
                mreq = sorted(mmeta.get('req', []))
                if sreq != mreq:
                    violations.append(f"CHECK-10: {tid}\n  [req mismatch] source={sreq} | map={mreq}")

    # PHASE 2 Reporting
    print(f"PHASE 2: METADATA VALIDATION ({'STRICT' if is_strict else 'PILOT'})\n")
    print(f"[1] SOURCE TESTS : {len(source_ids)}")
    print(f"[2] TAGGED       : {tagged_count}")
    print(f"[3] UNTAGGED     : {len(source_ids) - tagged_count}")
    print(f"[4] VIOLATIONS   : {len(violations)}")
    print(f"[5] TESTWORLD    : {testworld_violations}\n")

    if violations:
        print("STATUS: FAIL\n\nViolations:")
        for v in violations[:20]:
            print(f"- {v}")
        if len(violations) > 20:
            print(f"\n... and {len(violations)-20} more.")
        sys.exit(1)
    else:
        if tagged_count == len(source_ids) or not is_strict:
            status = "PASS" if tagged_count == len(source_ids) else "PASS (partial)"
            print(f"STATUS: {status}")
            sys.exit(0)
        else:
            print("STATUS: FAIL (Strict mode requires all tests to be tagged)")
            sys.exit(1)

if __name__ == '__main__':
    main()
