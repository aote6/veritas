with open('src/machine.rs', 'r') as f:
    content = f.read()

old = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx.current_object = frame.parent_object;
                        self.registers = frame.registers;
                        self.pc = frame.return_pc;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            _ => {}'''

new = '''            Instruction::Return => {
                match self.call_stack.pop() {
                    Some(frame) => {
                        self.ctx.current_object = frame.parent_object;
                        self.registers = frame.registers;
                        self.pc = frame.return_pc;
                    }
                    None => {
                        self.status = MachineStatus::Halted;
                        return Ok(());
                    }
                }
                if self.pc >= self.ram.len() { self.status = MachineStatus::Halted; }
                return Ok(());
            }
            Instruction::HostCall { call_id } => {
                // P27: HostCall统一收口
                match call_id {
                    0..=3 => { /* valid, handled by host */ }
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
            _ => {}'''

content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done')
