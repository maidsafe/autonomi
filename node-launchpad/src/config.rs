// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::connection_mode::ConnectionMode;
use crate::system::get_primary_mount_point;
use crate::{action::Action, mode::Scene};
use ant_node_manager::config::is_running_as_root;
use color_eyre::eyre::{Result, eyre};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use derive_deref::{Deref, DerefMut};
use ratatui::style::{Color, Modifier, Style};
use serde::{Deserialize, Serialize, de::Deserializer};
use std::collections::HashMap;
use std::path::PathBuf;

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
            Action::StatusActions(StatusActions::StartStopNode),
        ),
        (
            bind("<Ctrl-S>"),
            Action::StatusActions(StatusActions::StartStopNode),
        ),
        (
            bind("<Ctrl-Shift-s>"),
            Action::StatusActions(StatusActions::StartNodes),
        ),
        (
            bind("<Ctrl-x>"),
            Action::StatusActions(StatusActions::StopNodes),
        ),
        (
            bind("<Ctrl-X>"),
            Action::StatusActions(StatusActions::StopNodes),
        ),
        (
            bind("<Ctrl-Shift-x>"),
            Action::StatusActions(StatusActions::StopNodes),
        ),
        // Node management
        (bind("<+>"), Action::StatusActions(StatusActions::AddNode)),
        (
            bind("<->"),
            Action::StatusActions(StatusActions::RemoveNodes),
        ),
        (
            bind("<Delete>"),
            Action::StatusActions(StatusActions::TriggerRemoveNode),
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
            Action::StatusActions(StatusActions::TriggerNodeLogs),
        ),
        (
            bind("<L>"),
            Action::StatusActions(StatusActions::TriggerNodeLogs),
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

/// Where to store the Nodes data.
///
/// If `base_dir` is the primary mount point, we store in "<base_dir>/$HOME/user_data_dir/autonomi/node".
///
/// if not we store in "<base_dir>/autonomi/node".
///
/// If should_create is true, the directory will be created if it doesn't exists.
pub fn get_launchpad_nodes_data_dir_path(
    base_dir: &PathBuf,
    should_create: bool,
) -> Result<PathBuf> {
    let mut mount_point = PathBuf::new();
    let is_root = is_running_as_root();

    let data_directory: PathBuf = if *base_dir == get_primary_mount_point() {
        if is_root {
            // The root's data directory isn't accessible to the user `ant`, so we are using an
            // alternative default path that `ant` can access.
            #[cfg(unix)]
            {
                let default_data_dir_path = PathBuf::from("/var/antctl/services");
                debug!(
                    "Running as root; using default path {:?} for nodes data directory instead of primary mount point",
                    default_data_dir_path
                );
                default_data_dir_path
            }
            #[cfg(windows)]
            get_user_data_dir()?
        } else {
            get_user_data_dir()?
        }
    } else {
        base_dir.clone()
    };
    mount_point.push(data_directory);
    mount_point.push("autonomi");
    mount_point.push("node");
    if should_create {
        debug!("Creating nodes data dir: {:?}", mount_point.as_path());
        match std::fs::create_dir_all(mount_point.as_path()) {
            Ok(_) => debug!("Nodes {:?} data dir created successfully", mount_point),
            Err(e) => {
                error!(
                    "Failed to create nodes data dir in {:?}: {:?}",
                    mount_point, e
                );
                return Err(eyre!(
                    "Failed to create nodes data dir in {:?}",
                    mount_point
                ));
            }
        }
    }
    Ok(mount_point)
}

fn get_user_data_dir() -> Result<PathBuf> {
    dirs_next::data_dir().ok_or_else(|| eyre!("User data directory is not obtainable",))
}

/// Where to store the Launchpad config & logs.
///
pub fn get_launchpad_data_dir_path() -> Result<PathBuf> {
    let mut home_dirs =
        dirs_next::data_dir().ok_or_else(|| eyre!("Data directory is not obtainable"))?;
    home_dirs.push("autonomi");
    home_dirs.push("launchpad");
    std::fs::create_dir_all(home_dirs.as_path())?;
    Ok(home_dirs)
}

pub fn get_config_dir() -> Result<PathBuf> {
    // TODO: consider using dirs_next::config_dir. Configuration and data are different things.
    let config_dir = get_launchpad_data_dir_path()?.join("config");
    std::fs::create_dir_all(&config_dir)?;
    Ok(config_dir)
}

#[cfg(windows)]
pub async fn configure_winsw() -> Result<()> {
    let data_dir_path = get_launchpad_data_dir_path()?;
    ant_node_manager::helpers::configure_winsw(
        &data_dir_path.join("winsw.exe"),
        ant_node_manager::VerbosityLevel::Minimal,
    )
    .await?;
    Ok(())
}

#[cfg(not(windows))]
pub async fn configure_winsw() -> Result<()> {
    Ok(())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AppData {
    pub discord_username: String,
    pub nodes_to_start: usize,
    pub storage_mountpoint: Option<PathBuf>,
    pub storage_drive: Option<String>,
    pub connection_mode: Option<ConnectionMode>,
    pub port_from: Option<u32>,
    pub port_to: Option<u32>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            discord_username: "".to_string(),
            nodes_to_start: 1,
            storage_mountpoint: None,
            storage_drive: None,
            connection_mode: None,
            port_from: None,
            port_to: None,
        }
    }
}

impl AppData {
    pub fn load(custom_path: Option<PathBuf>) -> Result<Self> {
        let config_path = if let Some(path) = custom_path {
            path
        } else {
            get_config_dir()
                .map_err(|_| color_eyre::eyre::eyre!("Could not obtain config dir"))?
                .join("app_data.json")
        };

        if !config_path.exists() {
            return Ok(Self::default());
        }

        let data = std::fs::read_to_string(&config_path).map_err(|e| {
            error!("Failed to read app data file: {}", e);
            color_eyre::eyre::eyre!("Failed to read app data file: {}", e)
        })?;

        let app_data: AppData = serde_json::from_str(&data).map_err(|e| {
            error!("Failed to parse app data: {}", e);
            color_eyre::eyre::eyre!("Failed to parse app data: {}", e)
        })?;

        Ok(app_data)
    }

    pub fn save(&self, custom_path: Option<PathBuf>) -> Result<()> {
        let config_path = if let Some(path) = custom_path {
            path
        } else {
            get_config_dir()
                .map_err(|_| config::ConfigError::Message("Could not obtain data dir".to_string()))?
                .join("app_data.json")
        };

        let serialized_config = serde_json::to_string_pretty(&self)?;
        std::fs::write(config_path, serialized_config)?;

        Ok(())
    }
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

#[derive(Clone, Debug, Default, Deref, DerefMut, Serialize)]
pub struct Styles(pub HashMap<Scene, HashMap<String, Style>>);

impl<'de> Deserialize<'de> for Styles {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let parsed_map = HashMap::<Scene, HashMap<String, String>>::deserialize(deserializer)?;

        let styles = parsed_map
            .into_iter()
            .map(|(mode, inner_map)| {
                let converted_inner_map = inner_map
                    .into_iter()
                    .map(|(str, style)| (str, parse_style(&style)))
                    .collect();
                (mode, converted_inner_map)
            })
            .collect();

        Ok(Styles(styles))
    }
}

pub fn parse_style(line: &str) -> Style {
    let (foreground, background) =
        line.split_at(line.to_lowercase().find("on ").unwrap_or(line.len()));
    let foreground = process_color_string(foreground);
    let background = process_color_string(&background.replace("on ", ""));

    let mut style = Style::default();
    if let Some(fg) = parse_color(&foreground.0) {
        style = style.fg(fg);
    }
    if let Some(bg) = parse_color(&background.0) {
        style = style.bg(bg);
    }
    style = style.add_modifier(foreground.1 | background.1);
    style
}

fn process_color_string(color_str: &str) -> (String, Modifier) {
    let color = color_str
        .replace("grey", "gray")
        .replace("bright ", "")
        .replace("bold ", "")
        .replace("underline ", "")
        .replace("inverse ", "");

    let mut modifiers = Modifier::empty();
    if color_str.contains("underline") {
        modifiers |= Modifier::UNDERLINED;
    }
    if color_str.contains("bold") {
        modifiers |= Modifier::BOLD;
    }
    if color_str.contains("inverse") {
        modifiers |= Modifier::REVERSED;
    }

    (color, modifiers)
}

fn parse_color(s: &str) -> Option<Color> {
    let s = s.trim_start();
    let s = s.trim_end();
    if s.contains("bright color") {
        let s = s.trim_start_matches("bright ");
        let c = s
            .trim_start_matches("color")
            .parse::<u8>()
            .unwrap_or_default();
        Some(Color::Indexed(c.wrapping_shl(8)))
    } else if s.contains("color") {
        let c = s
            .trim_start_matches("color")
            .parse::<u8>()
            .unwrap_or_default();
        Some(Color::Indexed(c))
    } else if s.contains("gray") {
        let c = 232
            + s.trim_start_matches("gray")
                .parse::<u8>()
                .unwrap_or_default();
        Some(Color::Indexed(c))
    } else if s.contains("rgb") {
        let red = (s.as_bytes()[3] as char).to_digit(10).unwrap_or_default() as u8;
        let green = (s.as_bytes()[4] as char).to_digit(10).unwrap_or_default() as u8;
        let blue = (s.as_bytes()[5] as char).to_digit(10).unwrap_or_default() as u8;
        let c = 16 + red * 36 + green * 6 + blue;
        Some(Color::Indexed(c))
    } else if s == "bold black" {
        Some(Color::Indexed(8))
    } else if s == "bold red" {
        Some(Color::Indexed(9))
    } else if s == "bold green" {
        Some(Color::Indexed(10))
    } else if s == "bold yellow" {
        Some(Color::Indexed(11))
    } else if s == "bold blue" {
        Some(Color::Indexed(12))
    } else if s == "bold magenta" {
        Some(Color::Indexed(13))
    } else if s == "bold cyan" {
        Some(Color::Indexed(14))
    } else if s == "bold white" {
        Some(Color::Indexed(15))
    } else if s == "black" {
        Some(Color::Indexed(0))
    } else if s == "red" {
        Some(Color::Indexed(1))
    } else if s == "green" {
        Some(Color::Indexed(2))
    } else if s == "yellow" {
        Some(Color::Indexed(3))
    } else if s == "blue" {
        Some(Color::Indexed(4))
    } else if s == "magenta" {
        Some(Color::Indexed(5))
    } else if s == "cyan" {
        Some(Color::Indexed(6))
    } else if s == "white" {
        Some(Color::Indexed(7))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_parse_style_default() {
        let style = parse_style("");
        assert_eq!(style, Style::default());
    }

    #[test]
    fn test_parse_style_foreground() {
        let style = parse_style("red");
        assert_eq!(style.fg, Some(Color::Indexed(1)));
    }

    #[test]
    fn test_parse_style_background() {
        let style = parse_style("on blue");
        assert_eq!(style.bg, Some(Color::Indexed(4)));
    }

    #[test]
    fn test_parse_style_modifiers() {
        let style = parse_style("underline red on blue");
        assert_eq!(style.fg, Some(Color::Indexed(1)));
        assert_eq!(style.bg, Some(Color::Indexed(4)));
    }

    #[test]
    fn test_process_color_string() {
        let (color, modifiers) = process_color_string("underline bold inverse gray");
        assert_eq!(color, "gray");
        assert!(modifiers.contains(Modifier::UNDERLINED));
        assert!(modifiers.contains(Modifier::BOLD));
        assert!(modifiers.contains(Modifier::REVERSED));
    }

    #[test]
    fn test_parse_color_rgb() {
        let color = parse_color("rgb123");
        let expected = 16 + 36 + 2 * 6 + 3;
        assert_eq!(color, Some(Color::Indexed(expected)));
    }

    #[test]
    fn test_parse_color_unknown() {
        let color = parse_color("unknown");
        assert_eq!(color, None);
    }

    #[test]
    fn test_keybindings() -> Result<()> {
        let keybindings = get_keybindings();
        assert_eq!(
            keybindings
                .get(&Scene::Status)
                .unwrap()
                .get(&parse_key_sequence("<q>").unwrap_or_default())
                .unwrap(),
            &Action::Quit
        );
        Ok(())
    }

    #[test]
    fn test_app_data_file_does_not_exist() -> Result<()> {
        let temp_dir = tempdir()?;
        let non_existent_path = temp_dir.path().join("non_existent_app_data.json");

        let app_data = AppData::load(Some(non_existent_path))?;

        assert_eq!(app_data.discord_username, "");
        assert_eq!(app_data.nodes_to_start, 1);
        assert_eq!(app_data.storage_mountpoint, None);
        assert_eq!(app_data.storage_drive, None);
        assert_eq!(app_data.connection_mode, None);
        assert_eq!(app_data.port_from, None);
        assert_eq!(app_data.port_to, None);

        Ok(())
    }

    #[test]
    fn test_app_data_partial_info() -> Result<()> {
        let temp_dir = tempdir()?;
        let partial_data_path = temp_dir.path().join("partial_app_data.json");

        let partial_data = r#"
        {
            "discord_username": "test_user",
            "nodes_to_start": 3
        }
        "#;

        std::fs::write(&partial_data_path, partial_data)?;

        let app_data = AppData::load(Some(partial_data_path))?;

        assert_eq!(app_data.discord_username, "test_user");
        assert_eq!(app_data.nodes_to_start, 3);
        assert_eq!(app_data.storage_mountpoint, None);
        assert_eq!(app_data.storage_drive, None);
        assert_eq!(app_data.connection_mode, None);
        assert_eq!(app_data.port_from, None);
        assert_eq!(app_data.port_to, None);

        Ok(())
    }

    #[test]
    fn test_app_data_missing_mountpoint() -> Result<()> {
        let temp_dir = tempdir()?;
        let missing_mountpoint_path = temp_dir.path().join("missing_mountpoint_app_data.json");

        let missing_mountpoint_data = r#"
        {
            "discord_username": "test_user",
            "nodes_to_start": 3,
            "storage_drive": "C:"
        }
        "#;

        std::fs::write(&missing_mountpoint_path, missing_mountpoint_data)?;

        let app_data = AppData::load(Some(missing_mountpoint_path))?;

        assert_eq!(app_data.discord_username, "test_user");
        assert_eq!(app_data.nodes_to_start, 3);
        assert_eq!(app_data.storage_mountpoint, None);
        assert_eq!(app_data.storage_drive, Some("C:".to_string()));
        assert_eq!(app_data.connection_mode, None);
        assert_eq!(app_data.port_from, None);
        assert_eq!(app_data.port_to, None);

        Ok(())
    }

    #[test]
    fn test_app_data_save_and_load() -> Result<()> {
        let temp_dir = tempdir()?;
        let test_path = temp_dir.path().join("test_app_data.json");

        let mut app_data = AppData::default();
        let var_name = &"save_load_user";
        app_data.discord_username = var_name.to_string();
        app_data.nodes_to_start = 4;
        app_data.storage_mountpoint = Some(PathBuf::from("/mnt/test"));
        app_data.storage_drive = Some("E:".to_string());
        app_data.connection_mode = Some(ConnectionMode::CustomPorts);
        app_data.port_from = Some(12000);
        app_data.port_to = Some(13000);

        // Save to custom path
        app_data.save(Some(test_path.clone()))?;

        // Load from custom path
        let loaded_data = AppData::load(Some(test_path))?;

        assert_eq!(loaded_data.discord_username, "save_load_user");
        assert_eq!(loaded_data.nodes_to_start, 4);
        assert_eq!(
            loaded_data.storage_mountpoint,
            Some(PathBuf::from("/mnt/test"))
        );
        assert_eq!(loaded_data.storage_drive, Some("E:".to_string()));
        assert_eq!(
            loaded_data.connection_mode,
            Some(ConnectionMode::CustomPorts)
        );
        assert_eq!(loaded_data.port_from, Some(12000));
        assert_eq!(loaded_data.port_to, Some(13000));

        Ok(())
    }
}
