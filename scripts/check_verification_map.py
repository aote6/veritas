#!/usr/bin/env python3
import re
import sys
from pathlib import Path

TEST_PATTERN = re.compile(
    r"^[ \t]*#\[test\]\s*"
    r"(?:\n[ \t]*///.*)*\s*"
    r"(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(",
    re.MULTILINE,
)

def parse_rust_tests(repo_root: Path):
    tests = {}
    tests_dir = repo_root / "tests"
    
    for file_path in sorted(tests_dir.rglob("*.rs")):
        content = file_path.read_text(encoding="utf-8")
        rel_file = file_path.relative_to(repo_root)
        
        for test_name in TEST_PATTERN.findall(content):
            test_id = f"{rel_file}::{test_name}"
            if test_id in tests:
                print(f"[FATAL] Duplicate Test ID: {test_id}")
                sys.exit(1)
            tests[test_id] = {
                "file": str(rel_file),
                "name": test_name,
            }
    
    return tests

def parse_map_markdown(map_path: Path):
    mapped = set()
    
    if not map_path.exists():
        print(f"[FATAL] Map does not exist: {map_path}")
        sys.exit(1)
    
    for line in map_path.read_text(encoding="utf-8").splitlines():
        if "|" not in line:
            continue
        
        parts = [part.strip() for part in line.split("|")]
        if len(parts) < 3:
            continue
        
        test_id = parts[1]
        if not test_id:
            continue
        if test_id == "Test ID":
            continue
        if set(test_id) <= {"-", ":"}:
            continue
        
        mapped.add(test_id)
    
    return mapped

def audit_phase1(repo_root: Path):
    print("VERITAS VERIFICATION MAP AUDIT")
    print("================================")
    print("PHASE 1: TEST ID PARITY")
    print()
    
    actual = parse_rust_tests(repo_root)
    mapped = parse_map_markdown(
        repo_root / "docs" / "VERIFICATION_MAP.md"
    )
    
    actual_set = set(actual)
    mapped_set = set(mapped)
    
    missing = actual_set - mapped_set
    extra = mapped_set - actual_set
    
    print(f"[1] SOURCE TESTS : {len(actual_set)}")
    print(f"[2] MAP TESTS    : {len(mapped_set)}")
    print(f"[3] MISSING      : {len(missing)}")
    print(f"[4] EXTRA        : {len(extra)}")
    
    if missing:
        print()
        print("Missing from map:")
        for test_id in sorted(missing)[:30]:
            print(f" - {test_id}")
    
    if extra:
        print()
        print("Extra in map:")
        for test_id in sorted(extra)[:30]:
            print(f" - {test_id}")
    
    print()
    print("================================")
    
    if not missing and not extra:
        print("STATUS: PASS")
        print("Source and Map contain exactly the same Test IDs.")
        return 0
    
    print(
        f"STATUS: FAIL "
        f"(missing={len(missing)}, extra={len(extra)})"
    )
    return 1

def main():
    repo_root = Path(__file__).resolve().parent.parent
    
    if "--phase1" in sys.argv:
        sys.exit(audit_phase1(repo_root))
    
    print("Usage:")
    print("  python3 scripts/check_verification_map.py --phase1")
    sys.exit(2)

if __name__ == "__main__":
    main()
