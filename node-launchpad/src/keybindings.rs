// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::{
    action::{Action, NodeTableActions},
    mode::Scene,
};
use color_eyre::eyre::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use derive_deref::{Deref, DerefMut};
use serde::{Deserialize, Serialize, de::Deserializer};
use std::collections::HashMap;

pub fn get_keybindings() -> KeyBindings {
    use crate::{
        action::{Action, OptionsActions, StatusActions},
        mode::Scene,
    };
    use std::collections::HashMap;

    let bind = |key: &str| parse_key_sequence(key).unwrap();

    let add_common_bindings = |scene_map: &mut HashMap<_, _>| {
        scene_map.extend([
            // Scene navigation
            (bind("<s>"), Action::SwitchScene(Scene::Status)),
            (bind("<S>"), Action::SwitchScene(Scene::Status)),
            (bind("<o>"), Action::SwitchScene(Scene::Options)),
            (bind("<O>"), Action::SwitchScene(Scene::Options)),
            (bind("<h>"), Action::SwitchScene(Scene::Help)),
            (bind("<H>"), Action::SwitchScene(Scene::Help)),
            // Exit and suspend
            (bind("<q>"), Action::Quit),
            (bind("<Q>"), Action::Quit),
            (bind("<Shift-q>"), Action::Quit),
            (bind("<Ctrl-c>"), Action::Quit),
            (bind("<Ctrl-z>"), Action::Suspend),
        ]);
    };

    let mut keybindings = HashMap::new();

    // Status scene keybindings
    let mut status = HashMap::new();
    add_common_bindings(&mut status);
    status.extend([
        // Node control
        (
            bind("<Ctrl-s>"),
            Action::NodeTableActions(NodeTableActions::StartStopNode),
        ),
        (
            bind("<Ctrl-S>"),
            Action::NodeTableActions(NodeTableActions::StartStopNode),
        ),
        (
            bind("<Ctrl-Shift-s>"),
            Action::NodeTableActions(NodeTableActions::StartNodes),
        ),
        (
            bind("<Ctrl-x>"),
            Action::NodeTableActions(NodeTableActions::StopNodes),
        ),
        (
            bind("<Ctrl-X>"),
            Action::NodeTableActions(NodeTableActions::StopNodes),
        ),
        (
            bind("<Ctrl-Shift-x>"),
            Action::NodeTableActions(NodeTableActions::StopNodes),
        ),
        // Node management
        (
            bind("<+>"),
            Action::NodeTableActions(NodeTableActions::AddNode),
        ),
        (
            bind("<Shift-+>"),
            Action::NodeTableActions(NodeTableActions::AddNode),
        ),
        (
            bind("<->"),
            Action::NodeTableActions(NodeTableActions::RemoveNodes),
        ),
        (
            bind("<Delete>"),
            Action::NodeTableActions(NodeTableActions::TriggerRemoveNode),
        ),
        (
            bind("<Ctrl-g>"),
            Action::StatusActions(StatusActions::TriggerManageNodes),
        ),
        (
            bind("<Ctrl-G>"),
            Action::StatusActions(StatusActions::TriggerManageNodes),
        ),
        (
            bind("<Ctrl-Shift-g>"),
            Action::StatusActions(StatusActions::TriggerManageNodes),
        ),
        // Settings and logs
        (
            bind("<Ctrl-b>"),
            Action::StatusActions(StatusActions::TriggerRewardsAddress),
        ),
        (
            bind("<Ctrl-B>"),
            Action::StatusActions(StatusActions::TriggerRewardsAddress),
        ),
        (
            bind("<Ctrl-Shift-b>"),
            Action::StatusActions(StatusActions::TriggerRewardsAddress),
        ),
        (
            bind("<l>"),
            Action::NodeTableActions(NodeTableActions::TriggerNodeLogs),
        ),
        (
            bind("<L>"),
            Action::NodeTableActions(NodeTableActions::TriggerNodeLogs),
        ),
        // Navigation is now handled directly by NodeTableComponent
    ]);
    keybindings.insert(Scene::Status, status);

    // Options scene keybindings
    let mut options = HashMap::new();
    add_common_bindings(&mut options);
    options.extend([
        // Storage and connection
        (
            bind("<Ctrl-d>"),
            Action::OptionsActions(OptionsActions::TriggerChangeDrive),
        ),
        (
            bind("<Ctrl-D>"),
            Action::OptionsActions(OptionsActions::TriggerChangeDrive),
        ),
        (
            bind("<Ctrl-Shift-d>"),
            Action::OptionsActions(OptionsActions::TriggerChangeDrive),
        ),
        (
            bind("<Ctrl-k>"),
            Action::OptionsActions(OptionsActions::TriggerChangeConnectionMode),
        ),
        (
            bind("<Ctrl-K>"),
            Action::OptionsActions(OptionsActions::TriggerChangeConnectionMode),
        ),
        (
            bind("<Ctrl-Shift-k>"),
            Action::OptionsActions(OptionsActions::TriggerChangeConnectionMode),
        ),
        (
            bind("<Ctrl-p>"),
            Action::OptionsActions(OptionsActions::TriggerChangePortRange),
        ),
        (
            bind("<Ctrl-P>"),
            Action::OptionsActions(OptionsActions::TriggerChangePortRange),
        ),
        (
            bind("<Ctrl-Shift-p>"),
            Action::OptionsActions(OptionsActions::TriggerChangePortRange),
        ),
        // Settings
        (
            bind("<Ctrl-b>"),
            Action::OptionsActions(OptionsActions::TriggerRewardsAddress),
        ),
        (
            bind("<Ctrl-B>"),
            Action::OptionsActions(OptionsActions::TriggerRewardsAddress),
        ),
        (
            bind("<Ctrl-Shift-b>"),
            Action::OptionsActions(OptionsActions::TriggerRewardsAddress),
        ),
        (
            bind("<Ctrl-l>"),
            Action::OptionsActions(OptionsActions::TriggerAccessLogs),
        ),
        (
            bind("<Ctrl-L>"),
            Action::OptionsActions(OptionsActions::TriggerAccessLogs),
        ),
        (
            bind("<Ctrl-Shift-l>"),
            Action::OptionsActions(OptionsActions::TriggerAccessLogs),
        ),
        // Node operations
        (
            bind("<Ctrl-u>"),
            Action::OptionsActions(OptionsActions::TriggerUpdateNodes),
        ),
        (
            bind("<Ctrl-U>"),
            Action::OptionsActions(OptionsActions::TriggerUpdateNodes),
        ),
        (
            bind("<Ctrl-r>"),
            Action::OptionsActions(OptionsActions::TriggerResetNodes),
        ),
        (
            bind("<Ctrl-R>"),
            Action::OptionsActions(OptionsActions::TriggerResetNodes),
        ),
        (
            bind("<Ctrl-Shift-r>"),
            Action::OptionsActions(OptionsActions::TriggerResetNodes),
        ),
    ]);
    keybindings.insert(Scene::Options, options);

    // Help scene keybindings
    let mut help = HashMap::new();
    add_common_bindings(&mut help);
    keybindings.insert(Scene::Help, help);

    KeyBindings(keybindings)
}

#[derive(Clone, Debug, Default, Deref, DerefMut, Serialize)]
pub struct KeyBindings(pub HashMap<Scene, HashMap<Vec<KeyEvent>, Action>>);

impl<'de> Deserialize<'de> for KeyBindings {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parsed_map = HashMap::<Scene, HashMap<String, Action>>::deserialize(deserializer)?;

        let keybindings = parsed_map
            .into_iter()
            .map(|(mode, inner_map)| {
                let converted_inner_map = inner_map
                    .into_iter()
                    .map(|(key_str, cmd)| (parse_key_sequence(&key_str).unwrap(), cmd))
                    .collect();
                (mode, converted_inner_map)
            })
            .collect();

        Ok(KeyBindings(keybindings))
    }
}

fn parse_key_event(raw: &str) -> Result<KeyEvent, String> {
    let raw_lower = raw.to_ascii_lowercase();
    let (remaining, modifiers) = extract_modifiers(&raw_lower);
    parse_key_code_with_modifiers(remaining, modifiers)
}

fn extract_modifiers(raw: &str) -> (&str, KeyModifiers) {
    let mut modifiers = KeyModifiers::empty();
    let mut current = raw;

    loop {
        match current {
            rest if rest.starts_with("ctrl-") => {
                modifiers.insert(KeyModifiers::CONTROL);
                current = &rest[5..];
            }
            rest if rest.starts_with("alt-") => {
                modifiers.insert(KeyModifiers::ALT);
                current = &rest[4..];
            }
            rest if rest.starts_with("shift-") => {
                modifiers.insert(KeyModifiers::SHIFT);
                current = &rest[6..];
            }
            _ => break, // break out of the loop if no known prefix is detected
        };
    }

    (current, modifiers)
}

fn parse_key_code_with_modifiers(
    raw: &str,
    mut modifiers: KeyModifiers,
) -> Result<KeyEvent, String> {
    let c = match raw {
        "esc" => KeyCode::Esc,
        "enter" => KeyCode::Enter,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "backtab" => {
            modifiers.insert(KeyModifiers::SHIFT);
            KeyCode::BackTab
        }
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "insert" => KeyCode::Insert,
        "f1" => KeyCode::F(1),
        "f2" => KeyCode::F(2),
        "f3" => KeyCode::F(3),
        "f4" => KeyCode::F(4),
        "f5" => KeyCode::F(5),
        "f6" => KeyCode::F(6),
        "f7" => KeyCode::F(7),
        "f8" => KeyCode::F(8),
        "f9" => KeyCode::F(9),
        "f10" => KeyCode::F(10),
        "f11" => KeyCode::F(11),
        "f12" => KeyCode::F(12),
        "space" => KeyCode::Char(' '),
        "hyphen" => KeyCode::Char('-'),
        "minus" => KeyCode::Char('-'),
        "tab" => KeyCode::Tab,
        c if c.len() == 1 => {
            let mut c = c.chars().next().unwrap();
            if modifiers.contains(KeyModifiers::SHIFT) {
                c = c.to_ascii_uppercase();
            }
            KeyCode::Char(c)
        }
        _ => return Err(format!("Unable to parse {raw}")),
    };
    Ok(KeyEvent::new(c, modifiers))
}

pub fn key_event_to_string(key_event: &KeyEvent) -> String {
    let char;
    let key_code = match key_event.code {
        KeyCode::Backspace => "backspace",
        KeyCode::Enter => "enter",
        KeyCode::Left => "left",
        KeyCode::Right => "right",
        KeyCode::Up => "up",
        KeyCode::Down => "down",
        KeyCode::Home => "home",
        KeyCode::End => "end",
        KeyCode::PageUp => "pageup",
        KeyCode::PageDown => "pagedown",
        KeyCode::Tab => "tab",
        KeyCode::BackTab => "backtab",
        KeyCode::Delete => "delete",
        KeyCode::Insert => "insert",
        KeyCode::F(c) => {
            char = format!("f({c})");
            &char
        }
        KeyCode::Char(' ') => "space",
        KeyCode::Char(c) => {
            char = c.to_string();
            &char
        }
        KeyCode::Esc => "esc",
        KeyCode::Null => "",
        KeyCode::CapsLock => "",
        KeyCode::Menu => "",
        KeyCode::ScrollLock => "",
        KeyCode::Media(_) => "",
        KeyCode::NumLock => "",
        KeyCode::PrintScreen => "",
        KeyCode::Pause => "",
        KeyCode::KeypadBegin => "",
        KeyCode::Modifier(_) => "",
    };

    let mut modifiers = Vec::with_capacity(3);

    if key_event.modifiers.intersects(KeyModifiers::CONTROL) {
        modifiers.push("ctrl");
    }

    if key_event.modifiers.intersects(KeyModifiers::SHIFT) {
        modifiers.push("shift");
    }

    if key_event.modifiers.intersects(KeyModifiers::ALT) {
        modifiers.push("alt");
    }

    let mut key = modifiers.join("-");

    if !key.is_empty() {
        key.push('-');
    }
    key.push_str(key_code);

    key
}

pub fn parse_key_sequence(raw: &str) -> Result<Vec<KeyEvent>, String> {
    if raw.chars().filter(|c| *c == '>').count() != raw.chars().filter(|c| *c == '<').count() {
        return Err(format!("Unable to parse `{raw}`"));
    }
    let raw = if !raw.contains("><") {
        let raw = raw.strip_prefix('<').unwrap_or(raw);
        raw.strip_prefix('>').unwrap_or(raw)
    } else {
        raw
    };
    let sequences = raw
        .split("><")
        .map(|seq| {
            if let Some(s) = seq.strip_prefix('<') {
                s
            } else if let Some(s) = seq.strip_suffix('>') {
                s
            } else {
                seq
            }
        })
        .collect::<Vec<_>>();

    sequences.into_iter().map(parse_key_event).collect()
}
