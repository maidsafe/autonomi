// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::system;
use ant_node_manager::config::get_service_log_dir_path;
use ant_releases::ReleaseType;
use color_eyre::eyre::{self};
use ratatui::prelude::*;

/// helper function to create a centered rect using up certain percentage of the available rect `r`
pub fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(popup_layout[1])[1]
}

/// Helper function to create a centered rect using a fixed x,y constraint.
pub fn centered_rect_fixed(x: u16, y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(y),
        Constraint::Fill(1),
    ])
    .split(r);

    Layout::horizontal([
        Constraint::Fill(1),
        Constraint::Length(x),
        Constraint::Fill(1),
    ])
    .split(popup_layout[1])[1]
}

/// Opens the logs folder for a given node service name or the default service log directory.
///
/// # Parameters
///
/// * `node_name`: Optional node service name. If `None`, the default service log directory is used.
///
/// # Returns
///
/// A `Result` indicating the success or failure of the operation.
pub fn open_logs(node_name: Option<String>) -> Result<(), eyre::Report> {
    let service_path = get_service_log_dir_path(ReleaseType::NodeLaunchpad, None, None)?
        .to_string_lossy()
        .into_owned();

    let folder = if let Some(node_name) = node_name {
        format!("{service_path}/{node_name}")
    } else {
        service_path.to_string()
    };
    if let Err(e) = system::open_folder(&folder) {
        error!("Failed to open folder: {}", e);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centered_rect_produces_expected_size() {
        let area = Rect::new(0, 0, 100, 50);
        let rect = centered_rect(50, 40, area);
        assert_eq!(rect.width, 50);
        assert_eq!(rect.height, 20);
        assert_eq!(rect.x, 25);
        assert_eq!(rect.y, 15);
    }

    #[test]
    fn centered_rect_fixed_produces_expected_size() {
        let area = Rect::new(0, 0, 100, 50);
        let rect = centered_rect_fixed(40, 10, area);
        assert_eq!(rect.width, 40);
        assert_eq!(rect.height, 10);
        assert_eq!(rect.x, 30);
        assert_eq!(rect.y, 20);
    }
}
