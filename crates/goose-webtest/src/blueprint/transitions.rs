use serde::{Deserialize, Serialize};

use super::nodes::NodeStatus;

/// A transition between two nodes in the blueprint graph
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    pub from: String,
    pub to: String,
    #[serde(alias = "on")]
    pub condition: TransitionCondition,
}

/// Condition that must be met for a transition to fire
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum TransitionCondition {
    /// Transition on successful node execution
    OnSuccess,
    /// Transition on node failure
    OnFailure,
    /// Transition regardless of outcome
    Always,
    /// Transition based on a custom expression
    OnCondition { expression: String },
}

impl TransitionCondition {
    /// Check if this transition should fire given the node status
    pub fn matches(&self, status: &NodeStatus) -> bool {
        match self {
            TransitionCondition::OnSuccess => *status == NodeStatus::Pass,
            TransitionCondition::OnFailure => {
                *status == NodeStatus::Fail || *status == NodeStatus::Error
            }
            TransitionCondition::Always => true,
            TransitionCondition::OnCondition { .. } => {
                // TODO: Implement expression evaluation
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transition_matching() {
        assert!(TransitionCondition::OnSuccess.matches(&NodeStatus::Pass));
        assert!(!TransitionCondition::OnSuccess.matches(&NodeStatus::Fail));
        assert!(TransitionCondition::OnFailure.matches(&NodeStatus::Fail));
        assert!(TransitionCondition::OnFailure.matches(&NodeStatus::Error));
        assert!(TransitionCondition::Always.matches(&NodeStatus::Pass));
        assert!(TransitionCondition::Always.matches(&NodeStatus::Fail));
    }
}
