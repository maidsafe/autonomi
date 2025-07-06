// Copyright (C) 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::error::{Error, Result};
use service_manager::{
    ServiceInstallCtx, ServiceLabel, ServiceLevel, ServiceManager, ServiceStartCtx, ServiceStopCtx,
    ServiceUninstallCtx,
};
use std::{
    net::{SocketAddr, TcpListener},
    path::Path,
};
use sysinfo::System;

/// A thin wrapper around the `service_manager::ServiceManager`, which makes our own testing
/// easier.
///
/// We can make an assumption that this external component works correctly, so our own tests only
/// need assert that the service manager is used. Testing code that used the real service manager
/// would result in real services on the machines we are testing on; that can leave a bit of a mess
/// to clean up, especially if the tests fail.
pub trait ServiceControl: Sync {
    fn create_service_user(&self, username: &str) -> Result<()>;
    fn get_available_port(&self) -> Result<u16>;
    fn install(&self, install_ctx: ServiceInstallCtx, user_mode: bool) -> Result<()>;
    fn get_process_pid(&self, path: &Path) -> Result<u32>;
    fn start(&self, service_name: &str, user_mode: bool) -> Result<()>;
    fn stop(&self, service_name: &str, user_mode: bool) -> Result<()>;
    fn uninstall(&self, service_name: &str, user_mode: bool) -> Result<()>;
    fn wait(&self, delay: u64);
}

pub struct ServiceController {}

impl ServiceControl for ServiceController {
    #[cfg(target_os = "linux")]
    fn create_service_user(&self, username: &str) -> Result<()> {
        use std::process::Command;

        let output = Command::new("id")
            .arg("-u")
            .arg(username)
            .output()
            .inspect_err(|err| error!("Failed to execute id -u: {err:?}"))?;
        if output.status.success() {
            println!("The {username} user already exists");
            return Ok(());
        }

        let useradd_exists = Command::new("which")
            .arg("useradd")
            .output()
            .inspect_err(|err| error!("Failed to execute which useradd: {err:?}"))?
            .status
            .success();
        let adduser_exists = Command::new("which")
            .arg("adduser")
            .output()
            .inspect_err(|err| error!("Failed to execute which adduser: {err:?}"))?
            .status
            .success();

        let output = if useradd_exists {
            Command::new("useradd")
                .arg("-m")
                .arg("-s")
                .arg("/bin/bash")
                .arg(username)
                .output()
                .inspect_err(|err| error!("Failed to execute useradd: {err:?}"))?
        } else if adduser_exists {
            Command::new("adduser")
                .arg("-s")
                .arg("/bin/busybox")
                .arg("-D")
                .arg(username)
                .output()
                .inspect_err(|err| error!("Failed to execute adduser: {err:?}"))?
        } else {
            error!("Neither useradd nor adduser is available. ServiceUserAccountCreationFailed");
            return Err(Error::ServiceUserAccountCreationFailed);
        };

        if !output.status.success() {
            error!("Failed to create {username} user account: {output:?}");
            return Err(Error::ServiceUserAccountCreationFailed);
        }
        println!("Created {username} user account for running the service");
        info!("Created {username} user account for running the service");
        Ok(())
    }

    #[cfg(target_os = "macos")]
    fn create_service_user(&self, username: &str) -> Result<()> {
        use std::process::Command;
        use std::str;

        let output = Command::new("dscl")
            .arg(".")
            .arg("-list")
            .arg("/Users")
            .output()
            .inspect_err(|err| error!("Failed to execute dscl: {err:?}"))?;
        let output_str = str::from_utf8(&output.stdout)
            .inspect_err(|err| error!("Error while converting output to utf8: {err:?}"))?;
        if output_str.lines().any(|line| line == username) {
            return Ok(());
        }

        let output = Command::new("dscl")
            .arg(".")
            .arg("-list")
            .arg("/Users")
            .arg("UniqueID")
            .output()
            .inspect_err(|err| error!("Failed to execute dscl: {err:?}"))?;
        let output_str = str::from_utf8(&output.stdout)
            .inspect_err(|err| error!("Error while converting output to utf8: {err:?}"))?;
        let mut max_id = 0;

        for line in output_str.lines() {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() == 2 {
                if let Ok(id) = parts[1].parse::<u32>() {
                    if id > max_id {
                        max_id = id;
                    }
                }
            }
        }
        let new_unique_id = max_id + 1;

        let commands = vec![
            format!("dscl . -create /Users/{}", username),
            format!(
                "dscl . -create /Users/{} UserShell /usr/bin/false",
                username
            ),
            format!(
                "dscl . -create /Users/{} UniqueID {}",
                username, new_unique_id
            ),
            format!("dscl . -create /Users/{} PrimaryGroupID 20", username),
        ];
        for cmd in commands {
            let status = Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .status()
                .inspect_err(|err| error!("Error while executing dscl command: {err:?}"))?;
            if !status.success() {
                error!("The command {cmd} failed to execute. ServiceUserAccountCreationFailed");
                return Err(Error::ServiceUserAccountCreationFailed);
            }
        }
        Ok(())
    }

    #[cfg(target_os = "windows")]
    fn create_service_user(&self, _username: &str) -> Result<()> {
        Ok(())
    }

    fn get_available_port(&self) -> Result<u16> {
        let addr: SocketAddr = "127.0.0.1:0".parse()?;

        let socket = TcpListener::bind(addr)?;
        let port = socket.local_addr()?.port();
        drop(socket);
        trace!("Got available port: {port}");

        Ok(port)
    }

    fn get_process_pid(&self, bin_path: &Path) -> Result<u32> {
        debug!(
            "Searching for process with binary at {}",
            bin_path.to_string_lossy()
        );
        let system = System::new_all();
        for (pid, process) in system.processes() {
            if let Some(path) = process.exe() {
                if bin_path == path {
                    // There does not seem to be any easy way to get the process ID from the `Pid`
                    // type. Probably something to do with representing it in a cross-platform way.
                    trace!("Found process {bin_path:?} with PID: {pid}");
                    return Ok(pid.to_string().parse::<u32>()?);
                }
            }
        }
        error!(
            "No process was located with a path at {}",
            bin_path.to_string_lossy()
        );
        Err(Error::ServiceProcessNotFound(
            bin_path.to_string_lossy().to_string(),
        ))
    }

    fn install(&self, install_ctx: ServiceInstallCtx, user_mode: bool) -> Result<()> {
        debug!("Installing service: {install_ctx:?}");
        let mut manager = <dyn ServiceManager>::native()
            .inspect_err(|err| error!("Could not get native ServiceManage: {err:?}"))?;
        if user_mode {
            manager
                .set_level(ServiceLevel::User)
                .inspect_err(|err| error!("Could not set service to user mode: {err:?}"))?;
        }
        manager
            .install(install_ctx)
            .inspect_err(|err| error!("Error while installing service: {err:?}"))?;
        Ok(())
    }

    fn start(&self, service_name: &str, user_mode: bool) -> Result<()> {
        debug!("Starting service: {service_name}");
        let label: ServiceLabel = service_name.parse()?;
        let mut manager = <dyn ServiceManager>::native()
            .inspect_err(|err| error!("Could not get native ServiceManage: {err:?}"))?;
        if user_mode {
            manager
                .set_level(ServiceLevel::User)
                .inspect_err(|err| error!("Could not set service to user mode: {err:?}"))?;
        }
        manager
            .start(ServiceStartCtx { label })
            .inspect_err(|err| error!("Error while starting service: {err:?}"))?;
        Ok(())
    }

    fn stop(&self, service_name: &str, user_mode: bool) -> Result<()> {
        debug!("Stopping service: {service_name}");
        let label: ServiceLabel = service_name.parse()?;
        let mut manager = <dyn ServiceManager>::native()
            .inspect_err(|err| error!("Could not get native ServiceManage: {err:?}"))?;
        if user_mode {
            manager
                .set_level(ServiceLevel::User)
                .inspect_err(|err| error!("Could not set service to user mode: {err:?}"))?;
        }
        manager
            .stop(ServiceStopCtx { label })
            .inspect_err(|err| error!("Error while stopping service: {err:?}"))?;

        Ok(())
    }

    fn uninstall(&self, service_name: &str, user_mode: bool) -> Result<()> {
        debug!("Uninstalling service: {service_name}");
        let label: ServiceLabel = service_name.parse()?;
        let mut manager = <dyn ServiceManager>::native()
            .inspect_err(|err| error!("Could not get native ServiceManage: {err:?}"))?;

        if user_mode {
            manager
                .set_level(ServiceLevel::User)
                .inspect_err(|err| error!("Could not set service to user mode: {err:?}"))?;
        }
        match manager.uninstall(ServiceUninstallCtx { label }) {
            Ok(()) => Ok(()),
            Err(err) => {
                if std::io::ErrorKind::NotFound == err.kind() {
                    error!("Error while uninstall service, service file might have been removed manually: {service_name}");
                    // In this case the user has removed the service definition file manually,
                    // which the service manager crate treats as an error. We can propagate the
                    // it to the caller and they can decide how to handle it.
                    Err(Error::ServiceRemovedManually(service_name.to_string()))
                } else if err.raw_os_error() == Some(267) {
                    // This requires the unstable io_error_more feature, use raw code for now
                    // else if err.kind() == std::io::ErrorKind::NotADirectory {}

                    // This happens on windows when the service has been already cleared, but was not updated
                    // in the registry. Happens when the Service application (in windows) is open while calling
                    // 'remove' or 'reset'.
                    Err(Error::ServiceDoesNotExists(service_name.to_string()))
                } else {
                    error!("Error while uninstalling service: {err:?}");
                    Err(err.into())
                }
            }
        }
    }

    /// Provide a delay for the service to start or stop.
    ///
    /// This is wrapped mainly just for unit testing.
    fn wait(&self, delay: u64) {
        trace!("Waiting for {delay} milliseconds");
        std::thread::sleep(std::time::Duration::from_millis(delay));
    }
}
