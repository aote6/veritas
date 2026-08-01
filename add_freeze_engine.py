with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''    /// P8: OBJECT_DEATH
    pub fn object_death('''

new = '''    /// P26: OBJECT_FREEZE - 冻结Object，使其变为只读
    pub fn object_freeze(
        &self,
        ctx: &mut TransactionContext,
        object_id: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        let registry = self.object_registry.lock().unwrap();
        if !registry.contains_key(&object_id) {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        // 记录待冻结，Commit时生效
        ctx.pending_freezes.push(object_id);
        Ok(())
    }

    /// P8: OBJECT_DEATH
    pub fn object_death('''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine.rs')
