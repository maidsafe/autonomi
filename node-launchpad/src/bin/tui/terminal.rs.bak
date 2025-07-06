// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

#[cfg(target_os = "windows")]
use ant_node_manager::config::is_running_as_root;
use clap::Parser;
use color_eyre::eyre::{eyre, Result};
use std::{path::PathBuf, process::Command};
use which::which;

#[derive(Debug)]
pub(crate) enum TerminalType {
    Alacritty(PathBuf),
    Gnome(PathBuf),
    ITerm2(PathBuf),
    Kitty(PathBuf),
    Konsole(PathBuf),
    MacOS(PathBuf),
    WindowsCmd(PathBuf),
    WindowsPowershell(PathBuf),
    WindowsTerminal(PathBuf),
    Xterm(PathBuf),
}
pub(crate) fn detect_and_setup_terminal() -> Result<TerminalType> {
    #[cfg(target_os = "windows")]
    if !is_running_as_root() {
        {
            // TODO: There is no terminal to show this error message when double clicking on the exe.
            error!("Admin privileges required to run on Windows. Exiting.");
            color_eyre::eyre::bail!("Admin privileges required to run");
        }
    }

    if cfg!(target_os = "windows") {
        if let Ok(path) = which("wt.exe") {
            Ok(TerminalType::WindowsTerminal(path))
        } else if let Ok(path) = which("powershell.exe") {
            Ok(TerminalType::WindowsPowershell(path))
        } else if let Ok(path) = which("cmd.exe") {
            Ok(TerminalType::WindowsCmd(path))
        } else {
            Err(eyre!("Could not find suitable terminal on Windows"))
        }
    } else if cfg!(target_os = "macos") {
        if which("iTerm.app").is_ok() {
            Ok(TerminalType::ITerm2(PathBuf::from("osascript")))
        } else {
            Ok(TerminalType::MacOS(PathBuf::from("osascript")))
        }
    } else {
        get_available_linux_terminal()
    }
}

fn get_available_linux_terminal() -> Result<TerminalType> {
    if let Ok(path) = which("alacritty") {
        Ok(TerminalType::Alacritty(path))
    } else if let Ok(path) = which("gnome-terminal") {
        Ok(TerminalType::Gnome(path))
    } else if let Ok(path) = which("kitty") {
        Ok(TerminalType::Kitty(path))
    } else if let Ok(path) = which("konsole") {
        Ok(TerminalType::Konsole(path))
    } else if let Ok(path) = which("xterm") {
        Ok(TerminalType::Xterm(path))
    } else if let Ok(path) = which("xterm-256color") {
        Ok(TerminalType::Xterm(path))
    } else {
        Err(eyre!("Could not find terminal on Linux"))
    }
}

pub(crate) fn launch_terminal(terminal_type: &TerminalType) -> Result<()> {
    info!("Launching terminal: {terminal_type:?}");
    let launchpad_path = std::env::current_exe()?;

    match terminal_type {
        TerminalType::Kitty(path) | TerminalType::Konsole(path) | TerminalType::Xterm(path) => {
            Command::new(path).arg("-e").arg(launchpad_path).spawn()?;
            Ok(())
        }
        TerminalType::Alacritty(path) => {
            Command::new(path)
                .arg("--command")
                .arg("sh")
                .arg("-c")
                .arg(launchpad_path)
                .spawn()?;
            Ok(())
        }
        TerminalType::Gnome(path) => {
            Command::new(path).arg("--").arg(launchpad_path).spawn()?;
            Ok(())
        }
        TerminalType::MacOS(_path) | TerminalType::ITerm2(_path) => {
            // Mac automatically opens a new terminal window
            // so nothing to do here.
            Ok(())
        }
        TerminalType::WindowsTerminal(path) => {
            Command::new(path).arg(launchpad_path).spawn()?;
            Ok(())
        }
        TerminalType::WindowsCmd(path) | TerminalType::WindowsPowershell(path) => {
            Command::new(path).arg("/c").arg(launchpad_path).spawn()?;
            Ok(())
        }
    }
}

#[derive(Parser, Debug)]
#[command(author, about)]
pub struct Cli {
    #[arg(long)]
    pub launchpad_path: Option<PathBuf>,
    #[arg(long)]
    pub launchpad_version: Option<String>,
}
