with open('src/machine.rs', 'r') as f:
    content = f.read()

# Find where execute_instruction is called, add call_stack check before it for Commit
# The Commit check currently at line 404: if matches!(instruction, Instruction::Commit) {
# We need to check call_stack before execute_instruction when instruction is Commit

old = '''        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''

new = '''        // 宪法transaction.md第3节：Transaction不可嵌套。
        // CALL/RETURN不改变Transaction边界，Commit只能在最外层执行。
        if matches!(instruction, Instruction::Commit) && !self.call_stack.is_empty() {
            self.status = MachineStatus::Aborted(AbortReason::WriteConflict);
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }

        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''

content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
