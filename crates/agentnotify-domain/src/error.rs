/// 领域规则失败时返回的稳定错误。
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DomainError {
    #[error("标识无效: {kind}")]
    InvalidIdentifier { kind: &'static str },
    #[error("时间格式无效")]
    InvalidTimestamp,
    #[error("状态迁移无效")]
    InvalidStateTransition,
    #[error("回复路由已过期")]
    RouteExpired,
    #[error("找不到精确回复路由")]
    RouteMissing,
    #[error("回复路由存在冲突")]
    RouteConflict,
}

impl DomainError {
    /// 返回不随文案调整而变化的机器错误码。
    pub const fn code(self) -> &'static str {
        match self {
            Self::InvalidIdentifier { .. } => "invalid_identifier",
            Self::InvalidTimestamp => "invalid_timestamp",
            Self::InvalidStateTransition => "invalid_state_transition",
            Self::RouteExpired => "route_expired",
            Self::RouteMissing => "route_missing",
            Self::RouteConflict => "route_conflict",
        }
    }

    /// 返回可直接展示给用户的稳定中文文案。
    pub const fn message(self) -> &'static str {
        match self {
            Self::InvalidIdentifier { .. } => "标识格式无效",
            Self::InvalidTimestamp => "时间格式无效",
            Self::InvalidStateTransition => "当前状态不允许执行该操作",
            Self::RouteExpired => "回复路由已过期，请重新发送通知",
            Self::RouteMissing => "找不到对应的回复会话，请引用本次通知后回复",
            Self::RouteConflict => "回复路由存在冲突，请检查渠道账号和消息标识",
        }
    }
}
