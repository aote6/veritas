use crate::types::ObjectId;
use crate::view::ObjectView;
use crate::types::{VeritasError, AbortReason};

/// 物理生命周期统一防线。
/// 所有方法都是静态的，接收 &dyn ObjectView，不绑定任何具体存储。
pub struct ObjectGuard;

impl ObjectGuard {
    pub fn ensure_alive(view: &dyn ObjectView, id: ObjectId) -> Result<(), VeritasError> {
        if view.is_alive(id) {
            Ok(())
        } else {
            Err(VeritasError::Abort(AbortReason::WriteConflict))
        }
    }

    pub fn ensure_dead(view: &dyn ObjectView, id: ObjectId) -> Result<(), VeritasError> {
        if view.is_dead(id) {
            Ok(())
        } else {
            Err(VeritasError::Abort(AbortReason::WriteConflict))
        }
    }

    pub fn ensure_exists(view: &dyn ObjectView, id: ObjectId) -> Result<(), VeritasError> {
        if view.exists(id) {
            Ok(())
        } else {
            Err(VeritasError::Abort(AbortReason::WriteConflict))
        }
    }

    pub fn ensure_not_exists(view: &dyn ObjectView, id: ObjectId) -> Result<(), VeritasError> {
        if !view.exists(id) {
            Ok(())
        } else {
            Err(VeritasError::Abort(AbortReason::WriteConflict))
        }
    }

    pub fn ensure_linkable(
        view: &dyn ObjectView,
        from: ObjectId,
        to: ObjectId,
    ) -> Result<(), VeritasError> {
        if from == to {
            return Err(VeritasError::Abort(AbortReason::WriteConflict));
        }
        Self::ensure_alive(view, from)?;
        Self::ensure_alive(view, to)?;
        Ok(())
    }

    pub fn ensure_can_grant(
        view: &dyn ObjectView,
        grantee: ObjectId,
    ) -> Result<(), VeritasError> {
        Self::ensure_alive(view, grantee)
    }
}
