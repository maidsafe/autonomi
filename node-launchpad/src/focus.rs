// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

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

    pub fn set_focus(&mut self, target: FocusTarget) {
        if self.focus_stack.is_empty() {
            self.focus_stack.push(target);
        } else {
            *self.focus_stack.last_mut().unwrap() = target;
        }
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
