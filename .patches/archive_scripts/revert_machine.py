with open('src/machine.rs', 'r') as f:
    content = f.read()

# Revert WriteRegister - remove addr construction, pass state_id directly
content = content.replace(
    '''                let addr = crate::types::Address::new(self.ctx.current_object, state_id);
                self.executor.write_state(&mut self.ctx, addr, payload.clone())?;
                self.execution.record_write(addr, payload.clone());''',
    '''                self.executor.write_state(&mut self.ctx, state_id, payload.clone())?;
                self.execution.record_write(state_id, payload.clone());'''
)

# Revert LoadState instructions
content = content.replace(
    'let addr = crate::types::Address::new(self.ctx.current_object, state_id);\n                let bytes = self.executor.read_state(&mut self.ctx, addr)?;',
    'let bytes = self.executor.read_state(&mut self.ctx, state_id)?;'
)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: machine reverted to StateId')
