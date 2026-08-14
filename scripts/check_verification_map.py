#!/usr/bin/env python3
"""
scripts/check_verification_map.py
Veritas Verification Infrastructure - Single Source of Truth Auditor
"""

import sys
import re
from pathlib import Path

VALID_CATEGORIES = {"A", "B", "C", "D"}
VALID_POLICIES = {"FORBIDDEN", "PREFERRED", "ALLOWED", "ALLOWED_FOR_SETUP_ONLY"}
KNOWN_REQUIREMENTS = {
    "CAP-01", "CAP-02", "CAP-03", "CAP-04", "CAP-05",
    "WAL-01", "WAL-02", "TX-01", "REC-01", "MACH-01"
}

def parse_rust_tests(src_dir: Path):
    """Parse all #[test] functions and their doc comment tags."""
    tests = {}
    test_pattern = re.compile(
        r'((?:[ \t]*///.*\n)+)?[ \t]*#\[test\]\s+(?:async\s+)?fn\s+([a-zA-Z0-9_]+)\s*\(\s*\)\s*\{([^}]*)\}',
        re.MULTILINE
    )

    for file_path in src_dir.rglob("*.rs"):
        content = file_path.read_text(encoding="utf-8")
        for doc_comments, test_name, body in test_pattern.findall(content):
            meta = {
                "category": None,
                "layer": None,
                "testworld": None,
                "req": None,
                "body": body,
                "file": str(file_path)
            }
            if doc_comments:
                for line in doc_comments.split("\n"):
                    if "@category:" in line:
                        meta["category"] = line.split("@category:")[1].strip()
                    elif "@layer:" in line:
                        meta["layer"] = line.split("@layer:")[1].strip()
                    elif "@testworld:" in line:
                        meta["testworld"] = line.split("@testworld:")[1].strip()
                    elif "@req:" in line:
                        meta["req"] = line.split("@req:")[1].strip()

            if test_name in tests:
                print(f"[FATAL] Duplicate test name: {test_name}")
                sys.exit(1)
            tests[test_name] = meta

    return tests

def parse_map_markdown(map_path: Path):
    """Extract test IDs from VERIFICATION_MAP.md."""
    mapped_tests = set()
    if not map_path.exists():
        return mapped_tests

    for line in map_path.read_text(encoding="utf-8").split("\n"):
        if "|" in line:
            parts = [p.strip() for p in line.split("|")]
            if len(parts) > 2 and parts[1] and not parts[1].startswith("---") and parts[1] != "Test ID":
                mapped_tests.add(parts[1])
    return mapped_tests

def audit():
    repo_root = Path(__file__).parent.parent
    src_dir = repo_root / "tests"
    map_file = repo_root / "docs" / "VERIFICATION_MAP.md"

    print("VERITAS VERIFICATION MAP AUDIT")
    print("================================ ")

    actual_tests = parse_rust_tests(src_dir)
    mapped_tests = parse_map_markdown(map_file)

    actual_set = set(actual_tests.keys())
    violations = []

    missing = actual_set - mapped_tests
    extra = mapped_tests - actual_set

    cat_counts = {"A": 0, "B": 0, "C": 0, "D": 0, "UNTAGGED": 0}
    tw_violations = []

    for test_name, meta in actual_tests.items():
        cat = meta["category"]
        policy = meta["testworld"]
        req = meta["req"]
        body = meta["body"]

        if cat in VALID_CATEGORIES:
            cat_counts[cat] += 1
        else:
            cat_counts["UNTAGGED"] += 1
            violations.append(f"CHECK-03 Violation: [{test_name}] invalid or missing category: '{cat}'")

        if policy and policy not in VALID_POLICIES:
            violations.append(f"CHECK-04 Violation: [{test_name}] invalid policy: '{policy}'")

        if cat in {"B", "C"}:
            if "TestWorld" in body or "world." in body or "world_" in body:
                tw_violations.append(test_name)
                violations.append(f"CHECK-05 Violation: [{test_name}] Category {cat} uses TestWorld forbidden primitives!")

        if req and req not in KNOWN_REQUIREMENTS:
            violations.append(f"CHECK-06 Violation: [{test_name}] unknown requirement tagged: '{req}'")

    print(f"[1] ACTUAL TESTS discovered : {len(actual_set)}")
    print(f"[2] MAP TESTS mapped        : {len(mapped_tests)}")
    print(f"[3] SET EQUALITY            : missing={len(missing)} extra={len(extra)}")
    print(f"[4] TESTWORLD CONTAMINATION : {len(tw_violations)} violations")
    print(f"[5] CATEGORIES DISTRIBUTION : A:{cat_counts['A']} B:{cat_counts['B']} C:{cat_counts['C']} D:{cat_counts['D']} Untagged:{cat_counts['UNTAGGED']}")
    print("================================ ")

    if violations or missing or extra:
        print("[FAIL] Audit failed with issues:")
        for v in violations[:10]:
            print(f"  - {v}")
        if len(violations) > 10:
            print(f"  ... and {len(violations) - 10} more issues.")
        sys.exit(1)
    else:
        print("STATUS: PASS")
        sys.exit(0)

if __name__ == "__main__":
    audit()
