// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::eyre;
use color_eyre::eyre::ContextCompat;
use color_eyre::Result;
use faccess::{AccessMode, PathExt};

use std::env;

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use sysinfo::Disks;

// Tries to get the default (drive name, mount point) of the current executable
// to be used as the default drive
pub fn get_default_mount_point() -> Result<(String, PathBuf)> {
    // Create a new System instance
    let disks = Disks::new_with_refreshed_list();

    // Get the current executable path
    let exe_path = env::current_exe()?;

    // Iterate over the disks and find the one that matches the executable path
    for disk in disks.list() {
        if exe_path.starts_with(disk.mount_point()) {
            return Ok((
                disk.name().to_string_lossy().into(),
                disk.mount_point().to_path_buf(),
            ));
        }
    }
    Err(eyre!("Cannot find the default mount point"))
}

// Checks if the given path has read and write access
fn has_read_write_access(path: PathBuf) -> bool {
    let check_access = |mode, access_type| match path.access(mode) {
        Ok(_) => {
            debug!("{} access granted for {:?}", access_type, path);
            true
        }
        Err(_) => {
            debug!("{} access denied for {:?}", access_type, path);
            false
        }
    };

    let read = check_access(AccessMode::READ, "Read");
    let write = check_access(AccessMode::WRITE, "Write");

    read && write
}

/// Gets a list of available drives, their available space and if it's accessible.
///
/// An accessible drive is a drive that is readable and writable.
///
pub fn get_list_of_available_drives_and_available_space(
) -> Result<Vec<(String, PathBuf, u64, bool)>> {
    let disks = Disks::new_with_refreshed_list();
    let mut drives: Vec<(String, PathBuf, u64, bool)> = Vec::new();

    let default_mountpoint = match get_default_mount_point() {
        Ok((_name, mountpoint)) => mountpoint,
        Err(_) => PathBuf::new(),
    };

    for disk in disks.list() {
        let disk_info = (
            disk.name()
                .to_string_lossy()
                .into_owned()
                .trim()
                .to_string(),
            disk.mount_point().to_path_buf(),
            disk.available_space(),
            has_read_write_access(disk.mount_point().to_path_buf())
                || default_mountpoint == disk.mount_point().to_path_buf(),
        );

        // We avoid adding the same disk multiple times if it's mounted in multiple places
        // We check the name and free space to determine if it's the same disk
        if !drives
            .iter()
            .any(|drive| drive.0 == disk_info.0 && drive.2 == disk_info.2)
        {
            debug!("[ADD] Disk added: {:?}", disk_info);
            drives.push(disk_info);
        } else {
            debug!("[SKIP] Disk {:?} already added before.", disk_info);
        }
    }

    debug!("Drives detected: {:?}", drives);
    Ok(drives)
}

// Opens a folder in the file explorer
pub fn open_folder(path: &str) -> std::io::Result<()> {
    if Path::new(path).exists() {
        #[cfg(target_os = "macos")]
        Command::new("open").arg(path).spawn()?.wait()?;
        #[cfg(target_os = "windows")]
        Command::new("explorer").arg(path).spawn()?.wait()?;
        #[cfg(target_os = "linux")]
        Command::new("xdg-open").arg(path).spawn()?.wait()?;
    } else {
        error!("Path does not exist: {}", path);
    }
    Ok(())
}

#[cfg(unix)]
pub fn get_primary_mount_point() -> PathBuf {
    PathBuf::from("/")
}
#[cfg(windows)]
pub fn get_primary_mount_point() -> PathBuf {
    PathBuf::from("C:\\")
}

/// Gets the name of the primary mount point.
pub fn get_primary_mount_point_name() -> Result<String> {
    let primary_mount_point = get_primary_mount_point();
    let available_drives = get_list_of_available_drives_and_available_space()?;

    available_drives
        .iter()
        .find(|(_, mount_point, _, _)| mount_point == &primary_mount_point)
        .map(|(name, _, _, _)| name.clone())
        .ok_or_else(|| eyre!("Unable to find the name of the primary mount point"))
}

// Gets available disk space in bytes for the given mountpoint
pub fn get_available_space_b(storage_mountpoint: &PathBuf) -> Result<usize> {
    let disks = Disks::new_with_refreshed_list();
    if tracing::level_enabled!(tracing::Level::DEBUG) {
        for disk in disks.list() {
            let res = disk.mount_point() == storage_mountpoint;
            debug!(
                "Disk: {disk:?} is equal to '{:?}': {res:?}",
                storage_mountpoint,
            );
        }
    }

    let available_space_b = disks
        .list()
        .iter()
        .find(|disk| disk.mount_point() == storage_mountpoint)
        .context("Cannot find the primary disk. Configuration file might be wrong.")?
        .available_space() as usize;

    Ok(available_space_b)
}

// Gets the name of the drive given a mountpoint
pub fn get_drive_name(storage_mountpoint: &PathBuf) -> Result<String> {
    let disks = Disks::new_with_refreshed_list();
    let name = disks
        .list()
        .iter()
        .find(|disk| disk.mount_point() == storage_mountpoint)
        .context("Cannot find the primary disk. Configuration file might be wrong.")?
        .name()
        .to_str()
        .unwrap_or_default()
        .to_string();

    Ok(name)
}
