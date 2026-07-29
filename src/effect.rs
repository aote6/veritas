// Veritas Kernel - Effect 模块
// Phase 5: 副作用暂存与执行

use crate::types::PendingEffect;

/// 副作用队列：事务级暂存区
#[derive(Debug, Clone, Default)]
pub struct EffectQueue {
    pub effects: Vec<PendingEffect>,
}

impl EffectQueue {
    /// 添加副作用到队列
    pub fn push(&mut self, effect: PendingEffect) {
        self.effects.push(effect);
    }

    /// 取出所有副作用（消费）
    pub fn drain(&mut self) -> Vec<PendingEffect> {
        std::mem::take(&mut self.effects)
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }

    /// 队列长度
    pub fn len(&self) -> usize {
        self.effects.len()
    }
}
