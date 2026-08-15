#!/usr/bin/env python3
"""从测试源码提取元数据，生成审计工具期望的表格格式 Map。

依赖 check_verification_map.extract_source_tests 提取
@category / @layer / @testworld / @req 元数据，
输出 docs/VERIFICATION_MAP.md 表格供 check_verification_map.py 使用。
"""

import sys
from pathlib import Path

sys.path.insert(0, str(Path(__file__).parent))

from check_verification_map import extract_source_tests


def generate_map():
    tests, duplicates = extract_source_tests("tests")

    if duplicates:
        print("ERROR: duplicate test IDs found:")
        for d in sorted(set(duplicates)):
            print(f"  {d}")
        sys.exit(1)

    lines = [
        "# Veritas Verification Map",
        "",
        "## Test Inventory",
        "",
        "| Test ID | Category | Layer | TestWorld | Requirement |",
        "|---|---|---|---|---|",
    ]

    for test_id in sorted(tests):
        meta = tests[test_id].get("meta", {})
        category = meta.get("category", "")
        layer = meta.get("layer", "")
        testworld = meta.get("testworld", "")
        reqs = meta.get("req", [])
        req_str = ",".join(reqs) if reqs else ""
        lines.append(
            f"| {test_id} | {category} | {layer} | {testworld} | {req_str} |"
        )

    map_path = Path("docs/VERIFICATION_MAP.md")
    map_path.parent.mkdir(parents=True, exist_ok=True)
    map_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    print(f"Generated: {map_path}")
    print(f"Total tests: {len(tests)}")


if __name__ == "__main__":
    generate_map()
