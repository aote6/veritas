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

all_variants = set()
for m in re.finditer(r'^\s+(\w+)\s*\{', inst_src, re.MULTILINE):
    all_variants.add(m.group(1))
all_variants |= {'Halt','Nop','Commit','Return','Abort'}

step_start = machine_src.find('match instruction {')
step_end = machine_src.find('// 宪法transaction')
step_block = machine_src[step_start:step_end] if step_end > step_start else machine_src[step_start:]
step_handled = set(re.findall(r'Instruction::(\w+)', step_block))

eki_start = machine_src.find('fn execute_kernel_instruction')
eki_block = machine_src[eki_start:eki_start+200] if eki_start > 0 else ""
eki_handled = set(re.findall(r'Instruction::(\w+)', eki_block))

# Classification
trap_abi = {'ObjectBirth','ObjectDeath','ObjectFreeze','ObjectLink','ObjectUnlink'}
kernel_legacy = {'Read','Write'}

print(f"{'Instruction':<20} {'step()':<8} {'eki()':<8} {'Class':<18} {'Status'}")
print("-" * 70)

ok, legacy, missing, trap = 0, 0, 0, 0
for v in sorted(all_variants):
    s = "YES" if v in step_handled else " - "
    e = "YES" if v in eki_handled else " - "
    if v in trap_abi:
        cls = "Trap ABI (intended)"
        status = "OK — routed via Trap"
        trap += 1
    elif v in kernel_legacy:
        cls = "Kernel legacy"
        status = "OK — in eki()" if e == "YES" else "MISSING"
        legacy += 1
    elif s == "YES":
        cls = "CPU local"
        status = "OK"
        ok += 1
    elif e == "YES":
        cls = "CPU local"
        status = "LEGACY ONLY"
        legacy += 1
    else:
        cls = "UNKNOWN"
        status = "MISSING ❌"
        missing += 1
    print(f"{v:<20} {s:<8} {e:<8} {cls:<18} {status}")

print(f"\nOK: {ok}  Legacy: {legacy}  Trap ABI: {trap}  Missing: {missing}")
print(f"Total: {len(all_variants)} defined, {ok+legacy+trap} reachable")
sys.exit(0 if missing == 0 else 1)
