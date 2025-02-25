// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashMap;

use autonomi::{
    client::{
        files::{archive_private::PrivateArchiveDataMap, archive_public::ArchiveAddress},
        register::RegisterAddress,
        vault::UserData,
    },
    data::DataAddress,
};
use color_eyre::eyre::Result;

use super::data_dir::get_client_data_dir_path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PrivateFileArchive {
    name: String,
    secret_access: String,
}

pub fn get_local_user_data() -> Result<UserData> {
    let file_archives = get_local_public_file_archives()?;
    let private_file_archives = get_local_private_file_archives()?;
    let registers = get_local_registers()?;

    let user_data = UserData {
        file_archives,
        private_file_archives,
        register_addresses: registers,
    };
    Ok(user_data)
}

pub fn get_local_private_file_archives() -> Result<HashMap<PrivateArchiveDataMap, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_file_archives_path = user_data_path.join("private_file_archives");
    std::fs::create_dir_all(&private_file_archives_path)?;

    let mut private_file_archives = HashMap::new();
    for entry in walkdir::WalkDir::new(private_file_archives_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_content = std::fs::read_to_string(entry.path())?;
        let private_file_archive: PrivateFileArchive = serde_json::from_str(&file_content)?;
        let private_file_archive_access =
            PrivateArchiveDataMap::from_hex(&private_file_archive.secret_access)?;
        private_file_archives.insert(private_file_archive_access, private_file_archive.name);
    }
    Ok(private_file_archives)
}

pub fn get_local_private_archive_access(local_addr: &str) -> Result<PrivateArchiveDataMap> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_file_archives_path = user_data_path.join("private_file_archives");
    let file_path = private_file_archives_path.join(local_addr);
    let file_content = std::fs::read_to_string(file_path)?;
    let private_file_archive: PrivateFileArchive = serde_json::from_str(&file_content)?;
    let private_file_archive_access =
        PrivateArchiveDataMap::from_hex(&private_file_archive.secret_access)?;
    Ok(private_file_archive_access)
}

pub fn get_local_registers() -> Result<HashMap<RegisterAddress, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let registers_path = user_data_path.join("registers");
    std::fs::create_dir_all(&registers_path)?;

    let mut registers = HashMap::new();
    for entry in walkdir::WalkDir::new(registers_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let register_address = RegisterAddress::from_hex(&file_name)?;
        let file_content = std::fs::read_to_string(entry.path())?;
        let register_name = file_content;
        registers.insert(register_address, register_name);
    }
    Ok(registers)
}

pub fn get_name_of_local_register_with_address(address: &RegisterAddress) -> Result<String> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let registers_path = user_data_path.join("registers");
    let file_path = registers_path.join(address.to_hex());
    let file_content = std::fs::read_to_string(file_path)?;
    Ok(file_content)
}

pub fn get_local_public_file_archives() -> Result<HashMap<ArchiveAddress, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let file_archives_path = user_data_path.join("file_archives");
    std::fs::create_dir_all(&file_archives_path)?;

    let mut file_archives = HashMap::new();
    for entry in walkdir::WalkDir::new(file_archives_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let file_archive_address = DataAddress::from_hex(&file_name)?;
        let file_archive_name = std::fs::read_to_string(entry.path())?;
        file_archives.insert(file_archive_address, file_archive_name);
    }
    Ok(file_archives)
}

pub fn write_local_user_data(user_data: &UserData) -> Result<()> {
    for (archive, name) in user_data.file_archives.iter() {
        write_local_public_file_archive(archive.to_hex(), name)?;
    }

    for (archive, name) in user_data.private_file_archives.iter() {
        write_local_private_file_archive(archive.to_hex(), archive.address(), name)?;
    }

    for (register, name) in user_data.register_addresses.iter() {
        write_local_register(register, name)?;
    }

    Ok(())
}

pub fn write_local_register(register: &RegisterAddress, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let registers_path = user_data_path.join("registers");
    std::fs::create_dir_all(&registers_path)?;
    std::fs::write(registers_path.join(register.to_hex()), name)?;
    Ok(())
}

pub fn write_local_public_file_archive(archive: String, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let file_archives_path = user_data_path.join("file_archives");
    std::fs::create_dir_all(&file_archives_path)?;
    std::fs::write(file_archives_path.join(archive), name)?;
    Ok(())
}

pub fn write_local_private_file_archive(
    archive: String,
    local_addr: String,
    name: &str,
) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_file_archives_path = user_data_path.join("private_file_archives");
    std::fs::create_dir_all(&private_file_archives_path)?;
    let file_name = local_addr;
    let content = serde_json::to_string(&PrivateFileArchive {
        name: name.to_string(),
        secret_access: archive,
    })?;
    std::fs::write(private_file_archives_path.join(file_name), content)?;
    Ok(())
}
