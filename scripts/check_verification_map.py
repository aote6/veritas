#!/usr/bin/env python3
import sys
import re
from pathlib import Path

# ==================================================
# 1. 核心策略与契约 (Contract & Policies)
# ==================================================

VALID_CATEGORIES = {'A', 'B', 'C', 'D'}
VALID_LAYERS = {'kernel', 'capability', 'transaction', 'recovery', 'integration'}
VALID_TESTWORLDS = {'FORBIDDEN', 'ALLOWED', 'PREFERRED'}

# Category 对应的 TestWorld 使用策略
CATEGORY_TESTWORLD_POLICY = {
    'A': {'FORBIDDEN'},
    'B': {'FORBIDDEN'},
    'C': {'FORBIDDEN'},
    'D': {'ALLOWED', 'PREFERRED', 'FORBIDDEN'}
}

# ==================================================
# 2. Rust 解析引擎 (Robust Rust Parser)
# ==================================================

def clean_for_analysis(code_str):
    """
    去除代码中的注释和字符串，防止大括号匹配错位或 TestWorld 在注释中被误报。
    """
    # 移除块级注释
    code_str = re.sub(r'/\*.*?\*/', '', code_str, flags=re.DOTALL)
    # 移除行内注释
    code_str = re.sub(r'//.*', '', code_str)
    # 移除标准字符串
    code_str = re.sub(r'"(?:\\.|[^"\\])*"', '""', code_str)
    # 移除原始字符串 (r"...")
    code_str = re.sub(r'r#".*?"#', '""', code_str, flags=re.DOTALL)
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
        
        for i, line in enumerate(lines):
            if '#[test]' in line or '#[tokio::test]' in line:
                # 寻找函数声明
                fn_name = None
                j = i + 1
                while j < len(lines) and j <= i + 5:
                    l_str = lines[j].strip()
                    if l_str.startswith('//'):
                        j += 1
                        continue
                    # 匹配普通函数或 async 函数
                    m = re.match(r'^(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\(', l_str)
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
                    
                # 提取函数体（免疫嵌套大括号干扰）
                body_lines = []
                brace_count = 0
                started = False
                k = j
                while k < len(lines):
                    cl = clean_for_analysis(lines[k])
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
# 3. 核心检验流 (Execution & Validation)
# ==================================================

def main():
    if len(sys.argv) < 2 or sys.argv[1] not in ['--phase1', '--phase2', '--phase2-strict']:
        print("Usage: python3 scripts/check_verification_map.py [--phase1 | --phase2 | --phase2-strict]")
        sys.exit(2)
        
    mode = sys.argv[1]
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
