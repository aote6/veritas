with open('src/machine.rs', 'r') as f:
    content = f.read()

# Fix WriteRegister instruction
old = '''                self.executor.write_state(&mut self.ctx, state_id, payload.clone())?;
                self.execution.record_write(state_id, payload.clone());'''
new = '''                let addr = crate::types::Address::new(self.ctx.current_object, state_id);
                self.executor.write_state(&mut self.ctx, addr, payload.clone())?;
                self.execution.record_write(addr, payload.clone());'''
content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: machine.rs fixed')
