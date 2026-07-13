use everything_domain::{PermissionScope, PolicyDecision, ToolDefinition};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct ToolPolicy {
    decisions: BTreeMap<PermissionScope, PolicyDecision>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        let mut decisions = BTreeMap::new();
        decisions.insert(PermissionScope::WorkspaceRead, PolicyDecision::Allow);
        decisions.insert(
            PermissionScope::WorkspaceWrite,
            PolicyDecision::RequireApproval,
        );
        decisions.insert(
            PermissionScope::ProcessExecute,
            PolicyDecision::RequireApproval,
        );
        decisions.insert(PermissionScope::GitRead, PolicyDecision::Allow);
        decisions.insert(PermissionScope::GitWrite, PolicyDecision::RequireApproval);
        decisions.insert(PermissionScope::NetworkLocal, PolicyDecision::Deny);
        decisions.insert(PermissionScope::NetworkExternal, PolicyDecision::Deny);
        decisions.insert(PermissionScope::SystemInstall, PolicyDecision::Deny);
        Self { decisions }
    }
}

impl ToolPolicy {
    pub fn set(&mut self, scope: PermissionScope, decision: PolicyDecision) {
        self.decisions.insert(scope, decision);
    }

    pub fn decision(&self, scope: PermissionScope) -> PolicyDecision {
        self.decisions
            .get(&scope)
            .copied()
            .unwrap_or(PolicyDecision::Deny)
    }

    pub fn authorize(
        &self,
        definition: &ToolDefinition,
        approval_granted: bool,
    ) -> Result<(), PolicyViolation> {
        for scope in &definition.required_permissions {
            match self.decision(*scope) {
                PolicyDecision::Allow => {}
                PolicyDecision::RequireApproval if approval_granted => {}
                PolicyDecision::RequireApproval => {
                    return Err(PolicyViolation::ApprovalRequired(*scope));
                }
                PolicyDecision::Deny => return Err(PolicyViolation::Denied(*scope)),
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum PolicyViolation {
    ApprovalRequired(PermissionScope),
    Denied(PermissionScope),
}

impl std::fmt::Display for PolicyViolation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ApprovalRequired(scope) => write!(
                formatter,
                "permission '{}' requires explicit operator approval",
                scope.as_str()
            ),
            Self::Denied(scope) => {
                write!(
                    formatter,
                    "permission '{}' is denied by policy",
                    scope.as_str()
                )
            }
        }
    }
}

impl std::error::Error for PolicyViolation {}
