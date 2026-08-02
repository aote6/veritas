#!/usr/bin/env python3
import shutil, datetime

def backup(path):
    ts = datetime.datetime.now().strftime("%Y%m%d_%H%M%S")
    bak = f"{path}.bak_{ts}"
    shutil.copy(path, bak)
    print(f"[backup] {path} -> {bak}")

def apply(path, edits):
    with open(path, "r", encoding="utf-8") as f:
        content = f.read()
    backup(path)
    for desc, old, new in edits:
        count = content.count(old)
        if count != 1:
            print(f"[FAIL] {path}: '{desc}' 锚点出现 {count} 次,跳过整个文件")
            return False
        content = content.replace(old, new, 1)
        print(f"[OK] {path}: {desc}")
    with open(path, "w", encoding="utf-8") as f:
        f.write(content)
    return True

engine_edits = [
    ("加 has_link 只读查询方法",
     "    /// 只读查询：capability_graph 当前的 grant_sequence 计数器值。测试用于推算 cap_id。\n    pub fn capability_sequence(&self) -> u64 {\n        let cap_graph = self.capability_graph.lock().unwrap();\n        cap_graph.current_sequence()\n    }",
     "    /// 只读查询：capability_graph 当前的 grant_sequence 计数器值。测试用于推算 cap_id。\n    pub fn capability_sequence(&self) -> u64 {\n        let cap_graph = self.capability_graph.lock().unwrap();\n        cap_graph.current_sequence()\n    }\n\n    /// 只读查询：topology 中是否存在 from->to 这条边(任意 link_type)。测试用。\n    pub fn has_link(&self, from: ObjectId, to: ObjectId) -> bool {\n        let topo = self.topology.lock().unwrap();\n        topo.iter().any(|edge| edge.from == from && edge.to == to)\n    }"),
]

ok = apply("src/engine.rs", engine_edits)

if ok:
    print("\n先跑: cargo build --lib 2>&1 | tail -40")
else:
    print("\n锚点没对上,贴 [FAIL] 信息")
