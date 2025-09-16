// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::{OptionExt, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FocusTarget {
    Status,
    NodeTable,
    Options,
    Help,
    ManageNodesPopup,
    RemoveNodePopup,
    ChangeDrivePopup,
    ChangeConnectionModePopup,
    PortRangePopup,
    RewardsAddressPopup,
    ResetNodesPopup,
    UpgradeNodesPopup,
    UpgradeLaunchpadPopup,
    NodeLogsPopup,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventResult {
    Consumed,
    Ignored,
}

#[derive(Debug, Clone)]
pub struct FocusManager {
    focus_stack: Vec<FocusTarget>,
}

impl FocusManager {
    pub fn new(initial_focus: FocusTarget) -> Self {
        Self {
            focus_stack: vec![initial_focus],
        }
    }

    pub fn current_focus(&self) -> Option<&FocusTarget> {
        self.focus_stack.last()
    }

    pub fn has_focus(&self, target: &FocusTarget) -> bool {
        self.current_focus() == Some(target)
    }

    pub fn push_focus(&mut self, target: FocusTarget) {
        self.focus_stack.push(target);
    }

    pub fn pop_focus(&mut self) -> Option<FocusTarget> {
        if self.focus_stack.len() > 1 {
            self.focus_stack.pop()
        } else {
            None
        }
    }

    pub fn set_focus(&mut self, target: FocusTarget) -> Result<()> {
        if self.focus_stack.is_empty() {
            self.focus_stack.push(target);
        } else {
            let last = self
                .focus_stack
                .last_mut()
                .ok_or_eyre("Focus stack is not empty")?;
            *last = target;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.focus_stack.is_empty()
    }

    pub fn clear_and_set(&mut self, target: FocusTarget) {
        self.focus_stack.clear();
        self.focus_stack.push(target);
    }

    pub fn get_focus_stack(&self) -> &[FocusTarget] {
        &self.focus_stack
    }
}

impl Default for FocusManager {
    fn default() -> Self {
        Self::new(FocusTarget::Status)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_and_current_focus() {
        let mut manager = FocusManager::new(FocusTarget::Status);
        assert_eq!(manager.current_focus(), Some(&FocusTarget::Status));
        manager.push_focus(FocusTarget::Options);
        assert_eq!(manager.current_focus(), Some(&FocusTarget::Options));
    }

    #[test]
    fn pop_focus_preserves_root() {
        let mut manager = FocusManager::new(FocusTarget::Status);
        assert!(manager.pop_focus().is_none());
        manager.push_focus(FocusTarget::Help);
        assert_eq!(manager.pop_focus(), Some(FocusTarget::Help));
        assert_eq!(manager.current_focus(), Some(&FocusTarget::Status));
    }

    #[test]
    fn set_focus_replaces_top() {
        let mut manager = FocusManager::new(FocusTarget::Status);
        manager.push_focus(FocusTarget::Options);
        manager.set_focus(FocusTarget::Help).unwrap();
        assert_eq!(manager.current_focus(), Some(&FocusTarget::Help));
    }

    #[test]
    fn clear_and_set_resets_stack() {
        let mut manager = FocusManager::new(FocusTarget::Status);
        manager.push_focus(FocusTarget::Options);
        manager.clear_and_set(FocusTarget::NodeTable);
        assert_eq!(manager.get_focus_stack(), &[FocusTarget::NodeTable]);
    }
}
