import re

with open('src/machine.rs', 'r') as f:
    content = f.read()

start_marker = "if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {"
end_marker = "self.pc += consumed;"

start_idx = content.index(start_marker)
end_idx = content.index(end_marker, start_idx) + len(end_marker)

correct_block = '''if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {
            let reason = match e {
                VeritasError::Abort(r) => r,
                _ => AbortReason::WriteConflict,
            };
            self.engine.abort(&mut self.ctx, reason);
            self.status = MachineStatus::Aborted(reason);
            return Err(e);
        }

        if matches!(instruction, Instruction::Commit) {
            let current_object = self.ctx.current_object;
            self.ctx = self.engine.begin();
            self.ctx.current_object = current_object;
        }

        self.pc += consumed;'''

content = content[:start_idx] + correct_block + content[end_idx:]

with open('src/machine.rs', 'w') as f:
    f.write(content)

print("Block replaced cleanly")
