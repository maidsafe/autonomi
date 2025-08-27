// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use node_launchpad::components::header::{Header, SelectedMenuItem};
use ratatui::{Terminal, backend::TestBackend, style::Color, widgets::StatefulWidget};

#[test]
fn test_header_renders_with_status_selected() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut state = SelectedMenuItem::Status;
    terminal
        .draw(|f| {
            let header = Header::new();
            header.render(f.area(), f.buffer_mut(), &mut state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("Autonomi Node Launchpad"));
    assert!(content.contains("[S]tatus"));
    assert!(content.contains("[O]ptions"));
    assert!(content.contains("[H]elp"));
}

#[test]
fn test_header_renders_with_options_selected() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut state = SelectedMenuItem::Options;
    terminal
        .draw(|f| {
            let header = Header::new();
            header.render(f.area(), f.buffer_mut(), &mut state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("[O]ptions"));

    if let Some(options_pos) = content.find("[O]ptions") {
        let o_index = options_pos + 1;
        if o_index < buffer.content().len() {
            let o_cell = &buffer.content()[o_index];
            assert_ne!(o_cell.fg, Color::Rgb(248, 248, 242));
        }
    }
}

#[test]
fn test_header_renders_with_help_selected() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut state = SelectedMenuItem::Help;
    terminal
        .draw(|f| {
            let header = Header::new();
            header.render(f.area(), f.buffer_mut(), &mut state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    assert!(content.contains("[H]elp"));
}

#[test]
fn test_header_version_display() {
    let backend = TestBackend::new(80, 1);
    let mut terminal = Terminal::new(backend).unwrap();

    let mut state = SelectedMenuItem::Status;
    terminal
        .draw(|f| {
            let header = Header::new();
            header.render(f.area(), f.buffer_mut(), &mut state);
        })
        .unwrap();

    let buffer = terminal.backend().buffer();
    let content = buffer
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();

    let version = env!("CARGO_PKG_VERSION");
    assert!(content.contains(&format!("(v{version})")));
}
