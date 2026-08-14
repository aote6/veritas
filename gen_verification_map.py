#!/usr/bin/env python3
"""从测试文件的 //! 注释自动生成 docs/VERIFICATION_MAP.md"""

import os
import re
from datetime import date

TESTS_DIR = "tests"

def extract_doc_comments(filepath):
    comments = []
    with open(filepath, "r") as f:
        for line in f:
            line = line.strip()
            if line.startswith("//!"):
                text = line[3:].strip()
                if text:
                    comments.append(text)
    return comments

def extract_test_names(filepath):
    names = []
    with open(filepath, "r") as f:
        content = f.read()
    pattern = r'#\[test\]\s*\n\s*fn\s+(\w+)'
    names = re.findall(pattern, content)
    return names

def main():
    files = sorted([f for f in os.listdir(TESTS_DIR) if f.endswith(".rs")])
    sections = []
    total_tests = 0
    for fname in files:
        filepath = os.path.join(TESTS_DIR, fname)
        comments = extract_doc_comments(filepath)
        test_names = extract_test_names(filepath)
        total_tests += len(test_names)
        if comments or test_names:
            section = f"### {fname}\n\n"
            if comments:
                section += "**验证内容**:\n\n"
                for c in comments:
                    section += f"- {c}\n"
                section += "\n"
            if test_names:
                section += f"**测试函数** ({len(test_names)} 个):\n\n"
                for name in test_names:
                    section += f"- {name}\n"
            sections.append(section)

    today = date.today().isoformat()
    header = f"""# Veritas 验证地图（自动生成）

生成日期: {today}
生成方式: python3 gen_verification_map.py
数据来源: tests/*.rs 的 //! 文档注释和 #[test] 函数名

此文件由脚本自动生成。不要手动编辑。
如果发现缺了内容，应该修改对应测试文件的 //! 注释，然后重新生成。

---

"""

    with open("docs/VERIFICATION_MAP.md", "w") as f:
        f.write(header)
        f.write("\n---\n\n".join(sections))
        f.write("\n---\n\n")
        f.write("## 总数\n\n")
        f.write(f"- 测试文件: {len(files)} 个\n")
        f.write(f"- 测试函数: {total_tests} 个\n")

    print(f"Generated docs/VERIFICATION_MAP.md from {len(files)} test files, {total_tests} test functions")

if __name__ == "__main__":
    main()
