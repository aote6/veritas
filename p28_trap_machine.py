# Step 3: machine.rs - add Trap handling
with open('src/machine.rs', 'r') as f:
    content = f.read()

old = '''            Instruction::HostCall { call_id } => {
                // P27: HostCall统一收口
                match call_id {'''

new = '''            Instruction::Trap { service_id } => {
                // P28: TRAP统一内核服务调用
                // 参数通过寄存器传递：r0, r1, r2
                match service_id {
                    0 => { // OBJECT_BIRTH
                        let object_id = self.registers.get_u64(0);
                        self.engine.object_birth(&mut self.ctx, object_id)?;
                    }
                    1 => { // OBJECT_DEATH
                        let object_id = self.registers.get_u64(0);
                        self.engine.object_death(&mut self.ctx, object_id)?;
                    }
                    2 => { // OBJECT_LINK
                        let from = self.registers.get_u64(0);
                        let to = self.registers.get_u64(1);
                        let lt = self.registers.get_u64(2) as u8;
                        let link_type = match lt {
                            0 => crate::types::LinkType::DependsOn,
                            1 => crate::types::LinkType::Owns,
                            2 => crate::types::LinkType::References,
                            _ => {
                                self.status = MachineStatus::Trapped(
                                    crate::types::TrapReason::InvalidEncoding { pc: self.pc }
                                );
                                return Ok(());
                            }
                        };
                        self.engine.object_link(&mut self.ctx, from, to, link_type)?;
                    }
                    3 => { // OBJECT_UNLINK
                        let from = self.registers.get_u64(0);
                        let to = self.registers.get_u64(1);
                        self.engine.object_unlink(&mut self.ctx, from, to)?;
                    }
                    4 => { // OBJECT_FREEZE
                        let object_id = self.registers.get_u64(0);
                        self.engine.object_freeze(&mut self.ctx, object_id)?;
                    }
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
            Instruction::HostCall { call_id } => {
                // P27: HostCall统一收口
                match call_id {'''

content = content.replace(old, new)

with open('src/machine.rs', 'w') as f:
    f.write(content)

print('Done: machine.rs')
