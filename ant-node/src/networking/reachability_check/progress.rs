// Copyright 2025 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use super::dialer::DialManager;
use super::{MAX_CONCURRENT_DIALS, MAX_WORKFLOW_ATTEMPTS};
use crate::networking::driver::event::DIAL_BACK_DELAY;

/// Time-based progress calculator for reachability check workflow.
///
/// Progress is mapped equally across all workflow attempts:
/// - Each workflow gets an equal share of the total progress space
/// - The final workflow gets any remaining space to reach exactly 1.0
///
/// Each workflow consists entirely of dial attempts with time-based progress
#[derive(Debug, Clone)]
pub(crate) struct ProgressCalculator;

impl ProgressCalculator {
    /// Create a new progress calculator.
    pub(crate) fn new() -> Self {
        Self
    }

    /// Calculate the overall progress across all workflows directly from DialManager.
    ///
    /// Returns a value between 0.0 and 100.0, divided equally across workflow attempts.
    pub(crate) fn calculate_progress(&self, dial_manager: &DialManager) -> f64 {
        let current_workflow = dial_manager.current_workflow_attempt;

        let effective_workflow = current_workflow.min(MAX_WORKFLOW_ATTEMPTS);
        let workflow_base_percent =
            ((effective_workflow - 1) as f64 * 100.0) / MAX_WORKFLOW_ATTEMPTS as f64;

        let workflow_range_percent = if effective_workflow == MAX_WORKFLOW_ATTEMPTS {
            // For the last workflow, use remaining space to reach exactly 100
            100.0 - workflow_base_percent
        } else {
            100.0 / MAX_WORKFLOW_ATTEMPTS as f64
        };

        let workflow_progress = self.calculate_workflow_progress(dial_manager);

        let progress = (workflow_base_percent
            + (workflow_progress * workflow_range_percent / 100.0))
            .min(100.0);
        trace!(
            "Workflow base {workflow_base_percent:.1}%, range {workflow_range_percent:.1}%, progress {progress:.1}%"
        );

        if progress == 0.0 {
            return 1.0; // Ensure we never return 0 for progress - use 1 to indicate in progress
        }

        progress
    }

    /// Calculate progress within the current workflow (0.0 to 100.0).
    fn calculate_workflow_progress(&self, dial_manager: &DialManager) -> f64 {
        let ongoing_attempts = dial_manager.get_ongoing_dial_attempts();
        if ongoing_attempts.is_empty() {
            return 0.0;
        }

        let mut total_progress = 0.0;
        // Calculate progress for ongoing attempts
        for dial_state in ongoing_attempts.values() {
            let individual_progress =
                self.calculate_individual_dial_progress_from_state(dial_state);
            total_progress += individual_progress;
        }

        // Average across all concurrent slots (even if not all filled)
        let avg = (total_progress / MAX_CONCURRENT_DIALS as f64).min(100.0);

        trace!(
            "Progress for {} ongoing attempts, total progress: {total_progress:.1}, average progress: {avg:.1}%",
            ongoing_attempts.len()
        );
        avg
    }

    /// Calculate progress for an ongoing dial attempt based on its state and timing.
    fn calculate_individual_dial_progress_from_state(
        &self,
        state: &super::dialer::DialState,
    ) -> f64 {
        use super::dialer::DialState;
        match state {
            DialState::Initiated { .. } => 0.0,
            DialState::Connected { at } => {
                // Connected, waiting for dial-back: 0-DIAL_BACK_DELAY seconds
                // 20% base for connection + progress through dial-back wait (80%)
                let elapsed_secs = at.elapsed().as_secs();
                20.0 + (((elapsed_secs as f64) / DIAL_BACK_DELAY.as_secs() as f64).min(1.0) * 80.0)
            }
            DialState::DialBackReceived { .. } => {
                100.0 // Complete
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::networking::reachability_check::dialer::{DialManager, DialResult, DialState};
    use libp2p::PeerId;
    use std::time::{Duration, Instant};

    #[test]
    fn test_dial_state_progress() {
        let calculator = ProgressCalculator::new();

        // Test initiated state - always returns 0
        let at = Instant::now() - Duration::from_secs(15);
        let state = DialState::Initiated { at };

        let progress = calculator.calculate_individual_dial_progress_from_state(&state);
        assert_eq!(
            progress, 0.0,
            "Initiated state should always return 0.0 progress"
        );

        // Test connected state at 50% of dial-back delay
        let half_dial_back = Duration::from_secs(DIAL_BACK_DELAY.as_secs() / 2);
        let at = Instant::now() - half_dial_back;
        let state = DialState::Connected { at };

        let progress = calculator.calculate_individual_dial_progress_from_state(&state);
        let expected = 60.0; // 20% connection + 50% of dial-back wait = 60%
        assert!(
            (progress - expected).abs() < 2.0,
            "Connected at 50% should give ~60% total progress, got {progress}"
        );

        // Test dial-back received
        let state = DialState::DialBackReceived { at: Instant::now() };
        let progress = calculator.calculate_individual_dial_progress_from_state(&state);
        assert_eq!(
            progress, 100.0,
            "Dial-back received should give 100% progress"
        );
    }

    fn create_mock_dial_manager(
        workflow: usize,
        _ongoing: std::collections::HashMap<PeerId, DialState>,
        completed: std::collections::HashMap<PeerId, DialResult>,
    ) -> DialManager {
        use crate::networking::reachability_check::dialer::{Dialer, InitialContactsManager};

        // Note: We cannot easily set ongoing dial attempts in the mock because
        // the field is private and there's no public setter. The real usage
        // would populate this through normal dial manager operations.
        DialManager {
            current_workflow_attempt: workflow,
            dialer: Dialer::default(),
            all_dial_attempts: completed,
            initial_contacts_manager: InitialContactsManager::default(),
        }
    }

    #[test]
    fn test_workflow_boundaries() {
        let calculator = ProgressCalculator::new();

        // Test workflow 1 with no attempts (should be 1)
        let dial_manager = create_mock_dial_manager(
            1,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        let progress = calculator.calculate_progress(&dial_manager);
        assert_eq!(progress, 1.0, "Workflow 1 with no attempts should be 1.0");

        // Test workflow 2 start (should be around 33% for 3 workflow attempts)
        let dial_manager = create_mock_dial_manager(
            2,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        let progress = calculator.calculate_progress(&dial_manager);

        let workflow2_start = 100.0 / MAX_WORKFLOW_ATTEMPTS as f64; // Should be ~33.33%
        assert!(
            (progress - workflow2_start).abs() < 1.0,
            "Start of workflow 2 should be ~{workflow2_start:.1}%, got {progress:.1}%"
        );

        // Test workflow beyond expected range
        let dial_manager = create_mock_dial_manager(
            5,
            std::collections::HashMap::new(),
            std::collections::HashMap::new(),
        );
        let progress = calculator.calculate_progress(&dial_manager);

        // Should be high progress for beyond expected workflows
        assert!(
            progress >= 66.0, // Should be at least 66% (2/3 workflows complete)
            "Unexpected workflow should have high progress, got {progress:.1}%"
        );
        assert!(progress <= 100.0, "Progress should never exceed 100%");
    }

    #[test]
    fn test_individual_progress_calculations() {
        let calculator = ProgressCalculator::new();

        // Test connected state progress at various stages
        let test_cases = [
            (0, 20.0),    // Just connected: 20% progress
            (90, 60.0),   // Half way through dial-back: 20% + 50% of 80% = 60%
            (135, 80.0),  // 75% through dial-back: 20% + 75% of 80% = 80%
            (180, 100.0), // Full dial-back time: 20% + 100% of 80% = 100%
        ];

        for (elapsed_secs, expected_progress) in test_cases {
            let at = Instant::now() - Duration::from_secs(elapsed_secs);
            let state = DialState::Connected { at };
            let progress = calculator.calculate_individual_dial_progress_from_state(&state);
            assert!(
                (progress - expected_progress).abs() < 1.0,
                "Connected state at {elapsed_secs}s should give ~{expected_progress:.1}% progress, got {progress:.1}%"
            );
        }

        // Test initiated state progress - always returns 0.0 regardless of elapsed time
        let test_cases = [
            (0, 0.0),  // Just initiated: 0% progress
            (15, 0.0), // Any elapsed time: still 0% progress
            (30, 0.0), // Any elapsed time: still 0% progress
        ];

        for (elapsed_secs, expected_progress) in test_cases {
            let at = Instant::now() - Duration::from_secs(elapsed_secs);
            let state = DialState::Initiated { at };
            let progress = calculator.calculate_individual_dial_progress_from_state(&state);
            assert_eq!(
                progress, expected_progress,
                "Initiated state at {elapsed_secs}s should give {expected_progress:.1}% progress, got {progress:.1}%"
            );
        }
    }
}
