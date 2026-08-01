with open('src/executor.rs', 'r') as f:
    content = f.read()

old = '''            Instruction::Commit => {
                self.engine.commit(ctx)?;
            }'''

new = '''            Instruction::Commit => {
                // 宪法transaction.md第3节：Transaction不可嵌套。
                // CALL/RETURN不改变Transaction边界，Commit只能在最外层执行。
                if !ctx.call_stack.is_empty() {
                    return Err(VeritasError::Abort(AbortReason::WriteConflict));
                }
                self.engine.commit(ctx)?;
            }'''

content = content.replace(old, new)

with open('src/executor.rs', 'w') as f:
    f.write(content)

print('Done')
