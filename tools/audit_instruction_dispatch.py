#!/usr/bin/env python3
"""Veritas Instruction Dispatch Auditor
Compares Instruction enum vs Machine::step() dispatch coverage.
Run: python3 tools/audit_instruction_dispatch.py
"""
import re, sys

with open("src/instruction.rs") as f:
    inst_src = f.read()
with open("src/machine.rs") as f:
    machine_src = f.read()

# All Instruction variants from enum definition
all_variants = set()
for m in re.finditer(r'^\s+(\w+)\s*\{', inst_src, re.MULTILINE):
    all_variants.add(m.group(1))
all_variants |= {'Halt','Nop','Commit','Return','Abort'}

# step() match block — from 'match instruction {' to '// 宪法transaction'
step_start = machine_src.find('match instruction {')
step_end = machine_src.find('// 宪法transaction')
if step_end < 0:
    step_end = len(machine_src)
step_block = machine_src[step_start:step_end]
step_handled = set(re.findall(r'Instruction::(\w+)', step_block))

# execute_kernel_instruction — full function body
eki_start = machine_src.find('fn execute_kernel_instruction')
if eki_start > 0:
    # Find matching closing brace by counting
    depth = 0
    eki_end = eki_start
    for i in range(eki_start, len(machine_src)):
        if machine_src[i] == '{':
            depth += 1
        elif machine_src[i] == '}':
            depth -= 1
            if depth == 0:
                eki_end = i + 1
                break
    eki_block = machine_src[eki_start:eki_end]
    eki_handled = set(re.findall(r'Instruction::(\w+)', eki_block))
else:
    eki_block = ""
    eki_handled = set()

# Classification
trap_abi = {'ObjectBirth','ObjectDeath','ObjectFreeze','ObjectLink','ObjectUnlink'}
kernel_legacy = {'Read','Write'}

print("=== Veritas Instruction Dispatch Audit ===\n")
print(f"{'Instruction':<20} {'step()':<8} {'eki()':<8} {'Class':<22} {'Status'}")
print("-" * 75)

ok, legacy_ok, trap_ok, missing = 0, 0, 0, 0
for v in sorted(all_variants):
    s = "YES" if v in step_handled else " - "
    e = "YES" if v in eki_handled else " - "
    if v in trap_abi:
        cls = "Trap ABI"
        status = "OK — via Trap"
        trap_ok += 1
    elif v in kernel_legacy:
        cls = "Kernel legacy"
        if e == "YES":
            status = "OK — in execute_kernel_instruction()"
            legacy_ok += 1
        else:
            status = "MISSING ❌"
            missing += 1
    elif s == "YES":
        cls = "CPU local / Kernel API"
        status = "OK"
        ok += 1
    elif e == "YES":
        cls = "Kernel legacy"
        status = "OK — in execute_kernel_instruction()"
        legacy_ok += 1
    else:
        cls = "UNKNOWN"
        status = "MISSING ❌"
        missing += 1
    print(f"{v:<20} {s:<8} {e:<8} {cls:<22} {status}")

total_ok = ok + legacy_ok + trap_ok
print(f"\n{'─' * 75}")
print(f"Execution Classes:")
print(f"  CPU local / Kernel API:  {ok} dispatched in step()")
print(f"  Kernel legacy (eki):     {legacy_ok} dispatched in execute_kernel_instruction()")
print(f"  Trap ABI:                {trap_ok} routed via Trap ABI")
print(f"  Missing:                 {missing}")
print(f"  Total:                   {len(all_variants)} defined, {total_ok} reachable")
print(f"  Coverage:                {total_ok}/{len(all_variants)}")

if missing > 0:
    print(f"\n⚠️  WARNING: {missing} instruction(s) have no execution path!")
    sys.exit(1)
else:
    print(f"\n✅ All instructions reachable.")
    sys.exit(0)
