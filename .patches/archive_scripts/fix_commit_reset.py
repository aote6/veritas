with open('src/engine.rs', 'r') as f:
    content = f.read()

# Find the commit function and add ctx reset after successful commit
# Look for the end of commit where global_version is incremented
old = '''        self.global_version.fetch_add(1, Ordering::SeqCst);

        // P4: 固化 Object 到全局注册表'''

new = '''        self.global_version.fetch_add(1, Ordering::SeqCst);

        // P24: Commit成功后重建TransactionContext，保留current_object
        // 新的Transaction有新的snapshot、空的read_set/write_set/effect_queue
        let next_snapshot = self.global_version.load(Ordering::Acquire);
        let current_object = ctx.current_object;
        let next_tx_id = ctx.tx_id() + 1;
        *ctx = TransactionContext::new(next_tx_id, next_snapshot);
        ctx.current_object = current_object;

        // P4: 固化 Object 到全局注册表'''

content = content.replace(old, new)

with open('src/engine.rs', 'w') as f:
    f.write(content)

print('Done: commit now resets TransactionContext')
