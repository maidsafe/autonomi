// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use std::collections::HashMap;

use autonomi::{
    Pointer, PointerAddress, Scratchpad, ScratchpadAddress,
    chunk::DataMapChunk,
    client::{
        files::{archive_private::PrivateArchiveDataMap, archive_public::ArchiveAddress},
        register::RegisterAddress,
        vault::UserData,
    },
    data::DataAddress,
};
use color_eyre::eyre::Context;
use color_eyre::eyre::Result;

use super::data_dir::get_client_data_dir_path;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct PrivateFileArchive {
    name: String,
    secret_access: String,
}

#[derive(Serialize, Deserialize)]
struct PrivateFile {
    name: String,
    secret_access: String,
}

#[derive(Serialize, Deserialize)]
struct PublicFile {
    name: String,
    data_address: String,
}

pub fn get_local_user_data() -> Result<UserData> {
    let file_archives = get_local_public_file_archives()?;
    let private_file_archives = get_local_private_file_archives()?;
    let public_files = get_local_public_files()?;
    let private_files = get_local_private_files()?;
    let registers = get_local_registers()?;
    let register_key = super::keys::get_register_signing_key()
        .map(|k| k.to_hex())
        .ok();
    let scratchpad_key = super::keys::get_scratchpad_general_signing_key()
        .map(|k| k.to_hex())
        .ok();
    let pointer_key = super::keys::get_pointer_general_signing_key()
        .map(|k| k.to_hex())
        .ok();

    let user_data = UserData {
        file_archives,
        private_file_archives,
        register_addresses: registers,
        register_key,
        scratchpad_key,
        pointer_key,
        public_files,
        private_files,
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

pub fn get_local_private_file_access(local_addr: &str) -> Result<DataMapChunk> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_files_path = user_data_path.join("private_files");
    let file_path = private_files_path.join(local_addr);
    let file_content = std::fs::read_to_string(file_path)?;
    let private_file: PrivateFile = serde_json::from_str(&file_content)?;
    let private_file_access = DataMapChunk::from_hex(&private_file.secret_access)?;
    Ok(private_file_access)
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
    let UserData {
        file_archives,
        private_file_archives,
        register_addresses,
        register_key,
        scratchpad_key,
        pointer_key,
        public_files,
        private_files,
    } = user_data;

    for (archive, name) in file_archives.iter() {
        write_local_public_file_archive(archive.to_hex(), name)?;
    }

    for (archive, name) in private_file_archives.iter() {
        write_local_private_file_archive(archive.to_hex(), archive.address(), name)?;
    }

    for (data_address, name) in public_files.iter() {
        write_local_public_file(data_address.to_hex(), name)?;
    }

    for (private_datamap, name) in private_files.iter() {
        write_local_private_file(private_datamap.to_hex(), private_datamap.address(), name)?;
    }

    for (register, name) in register_addresses.iter() {
        write_local_register(register, name)?;
    }

    if let Some(register_key) = &register_key {
        let key = super::keys::parse_register_signing_key(register_key)
            .wrap_err("Failed to parse register signing key while writing to local user data")?;
        super::keys::create_register_signing_key_file(key).wrap_err(
            "Failed to create register signing key file while writing to local user data",
        )?;
    }

    if let Some(scratchpad_key) = &scratchpad_key {
        let key = super::keys::parse_scratchpad_signing_key(scratchpad_key)
            .wrap_err("Failed to parse scratchpad signing key while writing to local user data")?;
        super::keys::create_scratchpad_signing_key_file(key).wrap_err(
            "Failed to create scratchpad signing key file while writing to local user data",
        )?;
    }

    if let Some(pointer_key) = &pointer_key {
        let key = super::keys::parse_pointer_signing_key(pointer_key)
            .wrap_err("Failed to parse pointer signing key while writing to local user data")?;
        super::keys::create_pointer_signing_key_file(key).wrap_err(
            "Failed to create pointer signing key file while writing to local user data",
        )?;
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

pub fn write_local_private_file(datamap_hex: String, local_addr: String, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_files_path = user_data_path.join("private_files");
    std::fs::create_dir_all(&private_files_path)?;
    let file_name = local_addr;
    let content = serde_json::to_string(&PrivateFile {
        name: name.to_string(),
        secret_access: datamap_hex.to_string(),
    })?;
    std::fs::write(private_files_path.join(file_name), content)?;
    Ok(())
}

pub fn get_local_private_files() -> Result<HashMap<PrivateArchiveDataMap, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let private_files_path = user_data_path.join("private_files");
    let mut files = HashMap::new();
    if !private_files_path.exists() {
        return Ok(files);
    }

    for entry in walkdir::WalkDir::new(private_files_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let content = match std::fs::read_to_string(entry.path()) {
            Ok(content) => content,
            Err(_) => {
                continue;
            }
        };
        let private_file: PrivateFile = match serde_json::from_str(&content) {
            Ok(file) => file,
            Err(_) => {
                continue;
            }
        };
        let datamap = PrivateArchiveDataMap::from_hex(&private_file.secret_access)?;
        files.insert(datamap, private_file.name);
    }
    Ok(files)
}

pub fn write_local_public_file(data_address: String, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let public_files_path = user_data_path.join("public_files");
    std::fs::create_dir_all(&public_files_path)?;
    let file_name = data_address.clone();
    let content = serde_json::to_string(&PublicFile {
        name: name.to_string(),
        data_address,
    })?;
    std::fs::write(public_files_path.join(file_name), content)?;
    Ok(())
}

pub fn get_local_public_files() -> Result<HashMap<DataAddress, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let public_files_path = user_data_path.join("public_files");
    let mut files = HashMap::new();
    if !public_files_path.exists() {
        return Ok(files);
    }

    for entry in walkdir::WalkDir::new(public_files_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let content = std::fs::read_to_string(entry.path())?;
        let public_file: PublicFile = serde_json::from_str(&content)?;
        let data_address = DataAddress::from_hex(&public_file.data_address)?;
        files.insert(data_address, public_file.name);
    }
    Ok(files)
}

pub fn write_local_scratchpad(address: ScratchpadAddress, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let scratchpads_path = user_data_path.join("scratchpads");
    std::fs::create_dir_all(&scratchpads_path)?;
    std::fs::write(scratchpads_path.join(name), address.to_hex())?;
    Ok(())
}

pub fn get_local_scratchpads() -> Result<HashMap<String, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let scratchpads_path = user_data_path.join("scratchpads");
    std::fs::create_dir_all(&scratchpads_path)?;

    let mut scratchpads = HashMap::new();
    for entry in walkdir::WalkDir::new(scratchpads_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let scratchpad_address = std::fs::read_to_string(entry.path())?;
        scratchpads.insert(file_name.to_string(), scratchpad_address);
    }
    Ok(scratchpads)
}

pub fn write_local_pointer(address: PointerAddress, name: &str) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let pointers_path = user_data_path.join("pointers");
    std::fs::create_dir_all(&pointers_path)?;
    std::fs::write(pointers_path.join(name), address.to_hex())?;
    Ok(())
}

pub fn get_local_pointers() -> Result<HashMap<String, String>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let pointers_path = user_data_path.join("pointers");
    std::fs::create_dir_all(&pointers_path)?;

    let mut pointers = HashMap::new();
    for entry in walkdir::WalkDir::new(pointers_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let pointer_address = std::fs::read_to_string(entry.path())?;
        pointers.insert(file_name.to_string(), pointer_address);
    }
    Ok(pointers)
}

/// Write a pointer value to local user data for caching
pub fn write_local_pointer_value(name: &str, pointer: &Pointer) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let pointer_values_path = user_data_path.join("pointer_values");
    std::fs::create_dir_all(&pointer_values_path)?;

    let filename = format!("{name}.json");
    let serialized = serde_json::to_string(pointer)?;
    std::fs::write(pointer_values_path.join(filename), serialized)?;
    Ok(())
}

/// Get cached pointer value from local storage
pub fn get_local_pointer_value(name: &str) -> Result<Pointer> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let pointer_values_path = user_data_path.join("pointer_values");
    std::fs::create_dir_all(&pointer_values_path)?;

    let filename = format!("{name}.json");
    let file_content = std::fs::read_to_string(pointer_values_path.join(filename))?;
    let pointer: Pointer = serde_json::from_str(&file_content)?;
    Ok(pointer)
}

/// Get cached pointer values from local storage
pub fn get_local_pointer_values() -> Result<std::collections::HashMap<String, Pointer>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let pointer_values_path = user_data_path.join("pointer_values");
    std::fs::create_dir_all(&pointer_values_path)?;

    let mut pointer_values = std::collections::HashMap::new();
    for entry in walkdir::WalkDir::new(pointer_values_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let name = file_name.strip_suffix(".json").unwrap_or(&file_name);
        let file_content = std::fs::read_to_string(entry.path())?;
        if let Ok(pointer) = serde_json::from_str::<Pointer>(&file_content) {
            pointer_values.insert(name.to_string(), pointer);
        }
    }
    Ok(pointer_values)
}

/// Write a scratchpad value to local user data for caching
pub fn write_local_scratchpad_value(name: &str, scratchpad: &Scratchpad) -> Result<()> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let scratchpad_values_path = user_data_path.join("scratchpad_values");
    std::fs::create_dir_all(&scratchpad_values_path)?;

    let filename = format!("{name}.json");
    let serialized = serde_json::to_string(scratchpad)?;
    std::fs::write(scratchpad_values_path.join(filename), serialized)?;
    Ok(())
}

/// Get cached scratchpad value from local storage
pub fn get_local_scratchpad_value(name: &str) -> Result<Scratchpad> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let scratchpad_values_path = user_data_path.join("scratchpad_values");
    std::fs::create_dir_all(&scratchpad_values_path)?;

    let filename = format!("{name}.json");
    let file_content = std::fs::read_to_string(scratchpad_values_path.join(filename))?;
    let scratchpad: Scratchpad = serde_json::from_str(&file_content)?;
    Ok(scratchpad)
}

/// Get cached scratchpad values from local storage
pub fn get_local_scratchpad_values() -> Result<std::collections::HashMap<String, Scratchpad>> {
    let data_dir = get_client_data_dir_path()?;
    let user_data_path = data_dir.join("user_data");
    let scratchpad_values_path = user_data_path.join("scratchpad_values");
    std::fs::create_dir_all(&scratchpad_values_path)?;

    let mut scratchpad_values = std::collections::HashMap::new();
    for entry in walkdir::WalkDir::new(scratchpad_values_path)
        .min_depth(1)
        .max_depth(1)
    {
        let entry = entry?;
        let file_name = entry.file_name().to_string_lossy();
        let name = file_name.strip_suffix(".json").unwrap_or(&file_name);
        let file_content = std::fs::read_to_string(entry.path())?;
        if let Ok(scratchpad) = serde_json::from_str::<Scratchpad>(&file_content) {
            scratchpad_values.insert(name.to_string(), scratchpad);
        }
    }
    Ok(scratchpad_values)
}
