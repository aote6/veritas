# Step 3: Add HostCall handling in Machine::step()
with open('src/machine.rs', 'r') as f:
    content = f.read()

old = '''            Instruction::ObjectFreeze { object_id } => {
                self.executor.object_freeze(&mut self.ctx, *object_id)?;
                self.pc += consumed;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }'''

# Check if ObjectFreeze already in machine.rs
if 'ObjectFreeze' not in content:
    # ObjectFreeze not yet in machine step, add both
    old = '''            _ => {}
        }

        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''

    new = '''            Instruction::HostCall { call_id } => {
                // P27: HostCall统一收口——Veritas与外部世界的唯一闸门
                // 当前Rust实现直接处理，未来由Machine外部环境注入
                match call_id {
                    0 => { /* host_time */ }
                    1 => { /* host_random */ }
                    2 => { /* host_write */ }
                    3 => { /* host_read */ }
                    _ => {
                        self.status = MachineStatus::Trapped(
                            crate::types::TrapReason::InvalidEncoding { pc: self.pc }
                        );
                        return Ok(());
                    }
                }
                self.pc += consumed;
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            _ => {}
        }

        if let Err(e) = self.executor.execute_instruction(&mut self.ctx, &instruction) {'''

    content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: machine.rs')
