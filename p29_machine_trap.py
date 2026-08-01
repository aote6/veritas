with open('src/machine.rs', 'r') as f:
    content = f.read()

old = '''        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {
            let reason = match e {
                VeritasError::Abort(r) => r,
                _ => AbortReason::WriteConflict,
            };
            self.engine.abort(&mut self.ctx, reason);
            self.status = MachineStatus::Aborted(reason);
            return Err(e);
        }'''

new = '''        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {
            match e {
                VeritasError::PermissionDenied => {
                    self.status = MachineStatus::Trapped(
                        crate::types::TrapReason::AccessDenied { pc: self.pc }
                    );
                    return Ok(());
                }
                VeritasError::Abort(r) => {
                    self.engine.abort(&mut self.ctx, r);
                    self.status = MachineStatus::Aborted(r);
                    return Err(VeritasError::Abort(r));
                }
                _ => {
                    let reason = AbortReason::WriteConflict;
                    self.engine.abort(&mut self.ctx, reason);
                    self.status = MachineStatus::Aborted(reason);
                    return Err(e);
                }
            }
        }'''

content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: PermissionDenied now triggers AccessDenied trap')
