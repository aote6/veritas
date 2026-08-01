with open('src/engine.rs', 'r') as f:
    content = f.read()

old = '''        ctx.pending_links.push(LinkEdge { from, to, link_type: relation });
        Ok(())
    }

    /// P4: OBJECT_BIRTH 最小物理原语'''

new = '''        ctx.pending_links.push(LinkEdge { from, to, link_type: relation });
        Ok(())
    }

    /// P26: OBJECT_UNLINK - 移除Object间的Link
    pub fn object_unlink(
        &self,
        ctx: &mut TransactionContext,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), VeritasError> {
        if ctx.is_aborted() {
            return Err(VeritasError::Abort(AbortReason::AlreadyAborted));
        }
        // 标记Link待删除，Commit时生效
        ctx.pending_unlinks.push((from, to));
        Ok(())
    }

    /// P4: OBJECT_BIRTH 最小物理原语'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: engine.rs')
