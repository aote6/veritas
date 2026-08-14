#!/usr/bin/env python3
import re
from pathlib import Path

def extract_all_tests():
    repo_root = Path(".")
    tests = {}
    pattern = re.compile(
        r"^[ \t]*#\[test\]\s*"
        r"(?:\n[ \t]*///.*)*\s*"
        r"(?:async\s+)?fn\s+([A-Za-z0-9_]+)\s*\(",
        re.MULTILINE,
    )
    
    for file_path in sorted(Path("tests").rglob("*.rs")):
        content = file_path.read_text(encoding="utf-8")
        rel_file = file_path.relative_to(repo_root)
        
        for test_name in pattern.findall(content):
            test_id = f"{rel_file}::{test_name}"
            if test_id in tests:
                raise RuntimeError(f"Duplicate Test ID: {test_id}")
            tests[test_id] = {
                "file": str(rel_file),
                "name": test_name,
            }
    
    return tests

def generate_map():
    tests = extract_all_tests()
    lines = [
        "# Veritas Verification Map",
        "",
        "## Test Inventory",
        "",
        "| Test ID | Category | Layer | TestWorld | Requirement |",
        "|---|---|---|---|---|",
    ]
    
    for test_id in sorted(tests):
        lines.append(f"| {test_id} | | | | |")
    
    map_path = Path("docs/VERIFICATION_MAP.md")
    map_path.parent.mkdir(parents=True, exist_ok=True)
    map_path.write_text(
        "\n".join(lines) + "\n",
        encoding="utf-8",
    )
    
    print(f"Generated: {map_path}")
    print(f"Total tests: {len(tests)}")

if __name__ == "__main__":
    generate_map()
