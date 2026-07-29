// Veritas Kernel - P2 Step 1: CapabilityGraph 纯数据层
//
// 设计决策记录（不要在后续 Step 里重新讨论，除非出现真实反例）：
//
// 1. CapabilityId 确定性生成：Hash(grantor, grantee, resource, grant_sequence)。
//    grant_sequence 是本模块自己的单调计数器，不借用 global_version / tx_id，
//    避免同一事务内 grant→rollback→再grant 时哈希碰撞（详见对话记录）。
//
// 2. CapabilityGraph 是森林（每个 capability_id 对应一棵树），不是一般 DAG。
//    约束：每个 (capability_id, holder) 组合只会被插入一次，且插入后的
//    HolderRecord 永不物理删除（revoke 只翻转 active 标志）。
//    这两条约束的直接推论：不可能成环——因为若 to 是 from 的祖先，
//    to 必然已经被插入过，插入 to 时的"已在树中"检查会直接拒绝重复委派。
//    所以这里不需要额外的祖先遍历来防环。
//
// 3. revoke 的语义：
//    - 级联(cascade=true)：holder 自己 + 整个下游子树的 active 全部置 false。
//    - 非级联(cascade=false)：只把 holder 自己的 active 置 false，
//      下游子孙的 active 保持不变（标准文档原文："间接委托保留"）。
//    holds() 只看 (cap_id, holder) 自己的 active 标志，不做树遍历。
//    parent/children 索引仅用于"级联时要往下影响谁"，不用于判断当前是否持有。
//
// 4. 撤销记录不物理删除边和 holder 记录（只翻转 active），
//    是为了保留完整的委派历史用于审计追溯，代价是 CapabilityGraph
//    会随时间增长（原型阶段可接受，压缩/归档留给以后）。

use std::collections::{HashMap, HashSet};

use crate::types::{deterministic_hash, CapabilityId, ModuleId, StateId};

/// 受保护资源目前统一用 StateId 表示——能力总是围绕某个具体 State 的访问权。
/// 如果以后需要对 Scope 本身发放能力，再扩展这个类型，现在没有真实场景不做。
pub type ResourceId = StateId;

pub fn capability_id_of(
    grantor: ModuleId,
    grantee: ModuleId,
    resource: ResourceId,
    grant_sequence: u64,
) -> CapabilityId {
    deterministic_hash(&format!(
        "__cap__:{}:{}:{}:{}",
        grantor, grantee, resource, grant_sequence
    ))
}

#[derive(Debug, Clone, PartialEq)]
pub enum CapabilityError {
    NotFound,
    NotHolder,
    SelfDelegation,
    /// to 在这个 capability_id 下已经有过持有记录（无论当前是否 active），
    /// 森林约束不允许二次插入——这同时也是防环机制本身。
    AlreadyInTree,
}

#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    pub capability_type: String,
    pub granted_by: ModuleId,
    pub root_holder: ModuleId,
    pub resource: ResourceId,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DelegationEdge {
    pub from: ModuleId,
    pub to: ModuleId,
    pub capability_id: CapabilityId,
    pub cascade_on_revoke: bool,
}

#[derive(Debug, Clone)]
struct HolderRecord {
    active: bool,
    /// None 表示这是根节点（GRANT 直接产生的持有者）
    parent: Option<ModuleId>,
}

pub struct CapabilityGraph {
    grants: HashMap<CapabilityId, CapabilityInfo>,
    holders: HashMap<(CapabilityId, ModuleId), HolderRecord>,
    children: HashMap<(CapabilityId, ModuleId), HashSet<ModuleId>>,
    edges: Vec<DelegationEdge>,
    grant_sequence: u64,
}

impl CapabilityGraph {
    pub fn new() -> Self {
        CapabilityGraph {
            grants: HashMap::new(),
            holders: HashMap::new(),
            children: HashMap::new(),
            edges: Vec::new(),
            grant_sequence: 0,
        }
    }

    /// 恢复时用：直接指定起始 sequence（WAL 里 max_sequence + 1），
    /// 避免重启后新 GRANT 和历史记录撞车（与 tx_id_counter 恢复同一套逻辑）。
    pub fn with_starting_sequence(start_sequence: u64) -> Self {
        let mut g = Self::new();
        g.grant_sequence = start_sequence;
        g
    }

    pub fn current_sequence(&self) -> u64 {
        self.grant_sequence
    }

    /// 纯数据层的 GRANT：只生成 CapabilityId 并建立树根，不做任何事务/WAL 逻辑。
    pub fn grant(
        &mut self,
        capability_type: String,
        grantor: ModuleId,
        grantee: ModuleId,
        resource: ResourceId,
    ) -> CapabilityId {
        self.grant_sequence += 1;
        let cap_id = capability_id_of(grantor, grantee, resource, self.grant_sequence);

        self.grants.insert(
            cap_id,
            CapabilityInfo {
                capability_type,
                granted_by: grantor,
                root_holder: grantee,
                resource,
            },
        );
        self.holders.insert(
            (cap_id, grantee),
            HolderRecord {
                active: true,
                parent: None,
            },
        );
        cap_id
    }

    /// holder 当前是否持有该能力（不做树遍历，只看自己的 active 标志）
    pub fn holds(&self, cap_id: CapabilityId, holder: ModuleId) -> bool {
        self.holders
            .get(&(cap_id, holder))
            .map(|r| r.active)
            .unwrap_or(false)
    }

    pub fn info(&self, cap_id: CapabilityId) -> Option<&CapabilityInfo> {
        self.grants.get(&cap_id)
    }

    pub fn delegate(
        &mut self,
        cap_id: CapabilityId,
        from: ModuleId,
        to: ModuleId,
        cascade_on_revoke: bool,
    ) -> Result<(), CapabilityError> {
        if !self.grants.contains_key(&cap_id) {
            return Err(CapabilityError::NotFound);
        }
        if from == to {
            return Err(CapabilityError::SelfDelegation);
        }
        if !self.holds(cap_id, from) {
            return Err(CapabilityError::NotHolder);
        }
        // 森林约束：to 在这个 cap_id 下不能已经有过记录——
        // 这一条同时防止了"多父"和"成环"（见文件头注释的证明）。
        if self.holders.contains_key(&(cap_id, to)) {
            return Err(CapabilityError::AlreadyInTree);
        }

        self.holders.insert(
            (cap_id, to),
            HolderRecord {
                active: true,
                parent: Some(from),
            },
        );
        self.children
            .entry((cap_id, from))
            .or_default()
            .insert(to);
        self.edges.push(DelegationEdge {
            from,
            to,
            capability_id: cap_id,
            cascade_on_revoke,
        });
        Ok(())
    }

    /// cascade_override: Some(bool) 强制指定本次撤销是否级联，
    /// None 则按该 holder 被委派时记录的 cascade_on_revoke 设置执行；
    /// 根节点没有委派边（它是 GRANT 产生的），找不到边时默认级联
    /// （撤销 GRANT 本身，安全优先：不确定就当作影响范围更大处理）。
    pub fn revoke(
        &mut self,
        cap_id: CapabilityId,
        holder: ModuleId,
        cascade_override: Option<bool>,
    ) -> Result<(), CapabilityError> {
        if !self.holders.contains_key(&(cap_id, holder)) {
            return Err(CapabilityError::NotHolder);
        }

        let cascade = cascade_override.unwrap_or_else(|| {
            self.edges
                .iter()
                .rev()
                .find(|e| e.capability_id == cap_id && e.to == holder)
                .map(|e| e.cascade_on_revoke)
                .unwrap_or(true)
        });

        self.holders.get_mut(&(cap_id, holder)).unwrap().active = false;

        if cascade {
            self.deactivate_subtree(cap_id, holder);
        }
        Ok(())
    }

    fn deactivate_subtree(&mut self, cap_id: CapabilityId, node: ModuleId) {
        let kids: Vec<ModuleId> = self
            .children
            .get(&(cap_id, node))
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();

        for child in kids {
            if let Some(rec) = self.holders.get_mut(&(cap_id, child)) {
                rec.active = false;
            }
            self.deactivate_subtree(cap_id, child);
        }
    }
}

impl Default for CapabilityGraph {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grant_produces_deterministic_id_for_same_sequence() {
        let cap_a = capability_id_of(1, 2, 100, 1);
        let cap_b = capability_id_of(1, 2, 100, 1);
        assert_eq!(cap_a, cap_b);
    }

    #[test]
    fn test_grant_then_revoke_then_regrant_same_tuple_gives_different_ids() {
        let mut graph = CapabilityGraph::new();
        let cap1 = graph.grant("read".to_string(), 1, 2, 100);
        graph.revoke(cap1, 2, None).unwrap();
        let cap2 = graph.grant("read".to_string(), 1, 2, 100);
        assert_ne!(cap1, cap2);
    }

    #[test]
    fn test_holds_true_after_grant() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        assert!(graph.holds(cap, 2));
        assert!(!graph.holds(cap, 3));
    }

    #[test]
    fn test_delegate_requires_holder() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        let result = graph.delegate(cap, 99, 3, true);
        assert_eq!(result, Err(CapabilityError::NotHolder));
    }

    #[test]
    fn test_delegate_rejects_second_parent() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, true).unwrap();
        graph.delegate(cap, 2, 4, true).unwrap();
        let result = graph.delegate(cap, 4, 3, true);
        assert_eq!(result, Err(CapabilityError::AlreadyInTree));
    }

    #[test]
    fn test_delegate_rejects_cycle_via_already_in_tree() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, true).unwrap();
        graph.delegate(cap, 3, 4, true).unwrap();
        let result = graph.delegate(cap, 4, 2, true);
        assert_eq!(result, Err(CapabilityError::AlreadyInTree));
    }

    #[test]
    fn test_self_delegation_rejected() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        let result = graph.delegate(cap, 2, 2, true);
        assert_eq!(result, Err(CapabilityError::SelfDelegation));
    }

    #[test]
    fn test_cascade_revoke_deactivates_whole_subtree() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, true).unwrap();
        graph.delegate(cap, 3, 4, true).unwrap();

        graph.revoke(cap, 3, None).unwrap();

        assert!(graph.holds(cap, 2));
        assert!(!graph.holds(cap, 3));
        assert!(!graph.holds(cap, 4));
    }

    #[test]
    fn test_non_cascade_revoke_preserves_downstream() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, false).unwrap();
        graph.delegate(cap, 3, 4, true).unwrap();

        graph.revoke(cap, 3, None).unwrap();

        assert!(graph.holds(cap, 2));
        assert!(!graph.holds(cap, 3));
        assert!(graph.holds(cap, 4));
    }

    #[test]
    fn test_revoke_override_forces_cascade_regardless_of_edge_setting() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, false).unwrap();
        graph.delegate(cap, 3, 4, true).unwrap();

        graph.revoke(cap, 3, Some(true)).unwrap();

        assert!(!graph.holds(cap, 3));
        assert!(!graph.holds(cap, 4));
    }

    #[test]
    fn test_revoke_root_defaults_to_cascade() {
        let mut graph = CapabilityGraph::new();
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        graph.delegate(cap, 2, 3, false).unwrap();

        graph.revoke(cap, 2, None).unwrap();

        assert!(!graph.holds(cap, 2));
        assert!(!graph.holds(cap, 3));
    }

    #[test]
    fn test_with_starting_sequence_avoids_collision_after_restart() {
        let mut graph = CapabilityGraph::with_starting_sequence(5);
        let cap = graph.grant("read".to_string(), 1, 2, 100);
        let expected = capability_id_of(1, 2, 100, 6);
        assert_eq!(cap, expected);
    }
}
