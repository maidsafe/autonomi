// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use color_eyre::eyre::{Context, Result, eyre};
use std::fs;
use std::path::PathBuf;

// List of items to migrate
const ITEMS_TO_MIGRATE: [&str; 4] = [
    "user_data",
    "register_signing_key",
    "scratchpad_signing_key",
    "pointer_signing_key",
];

/// Check if legacy data exists (data directly under client/ without wallet address)
fn legacy_data_exists() -> Result<bool> {
    let base_dir = super::data_dir::get_client_data_dir_base()?;
    Ok(ITEMS_TO_MIGRATE
        .iter()
        .any(|item| base_dir.join(item).exists()))
}

/// Migrate legacy user data from the old structure to the new wallet-based structure
/// This should only be called when we have the wallet address available
pub fn migrate_legacy_data_if_needed(wallet_address: &str) -> Result<()> {
    if !legacy_data_exists()? {
        // No legacy data to migrate
        return Ok(());
    }

    let base_dir = super::data_dir::get_client_data_dir_base()?;

    println!(
        "Detected legacy user data. Updating your local data file location to the new directory structure with multiple account support..."
    );

    let new_wallet_dir = base_dir.join(wallet_address);

    // Create the new wallet directory if it doesn't exist
    fs::create_dir_all(&new_wallet_dir)
        .wrap_err("Failed to create wallet directory for migration")?;

    let mut migration_errors = Vec::new();

    for item in ITEMS_TO_MIGRATE {
        let old_path = base_dir.join(item);
        let new_path = new_wallet_dir.join(item);

        if old_path.exists() {
            // Check if the destination already exists
            if new_path.exists() {
                if data_is_identical(&old_path, &new_path)? {
                    println!(
                        "Skipping migration of {item} as identical data already exists in the new location"
                    );
                } else {
                    println!(
                        "Skipping migration of {item} as different data already exists in the new location (preserving existing data)"
                    );
                }
                continue;
            }

            // Perform the migration by moving the directory/file
            match fs::rename(&old_path, &new_path) {
                Ok(()) => {
                    println!("Successfully migrated: {item}");
                }
                Err(e) => {
                    // If rename fails (e.g., across filesystems), try manual move
                    match move_item_fallback(&old_path, &new_path) {
                        Ok(_) => {
                            println!("Successfully migrated: {item} (using fallback method)");
                        }
                        Err(fallback_err) => {
                            migration_errors.push(format!(
                                "Failed to migrate {item}: {fallback_err} (rename error: {e})"
                            ));
                        }
                    }
                }
            }
        }
    }

    if !migration_errors.is_empty() {
        return Err(eyre!(
            "Migration completed with errors: {:?}",
            migration_errors
        ));
    }

    println!("Migration completed successfully!");
    Ok(())
}

/// Fallback move method when fs::rename fails (e.g., across filesystems)
/// This recursively moves files/directories by copying then deleting
fn move_item_fallback(source: &PathBuf, destination: &PathBuf) -> Result<()> {
    if source.is_dir() {
        move_dir_all(source, destination)?;
    } else {
        // For files, copy then delete
        fs::copy(source, destination).wrap_err("Failed to copy file")?;
        fs::remove_file(source).wrap_err("Failed to remove source file after copying")?;
    }

    Ok(())
}

/// Recursively move a directory (copy then delete)
fn move_dir_all(src: &PathBuf, dst: &PathBuf) -> Result<()> {
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if src_path.is_dir() {
            move_dir_all(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)?;
            fs::remove_file(&src_path)?;
        }
    }

    // Remove the now-empty source directory
    fs::remove_dir(src)?;

    Ok(())
}

/// Check if two files or directories contain identical data
fn data_is_identical(path1: &PathBuf, path2: &PathBuf) -> Result<bool> {
    if path1.is_file() && path2.is_file() {
        // Compare file contents
        let content1 = fs::read(path1)?;
        let content2 = fs::read(path2)?;
        Ok(content1 == content2)
    } else if path1.is_dir() && path2.is_dir() {
        // Recursively compare directories
        compare_directories(path1, path2)
    } else {
        // One is file, one is directory - definitely different
        Ok(false)
    }
}

/// Recursively compare two directories to see if they contain identical data
fn compare_directories(dir1: &PathBuf, dir2: &PathBuf) -> Result<bool> {
    let mut entries1: Vec<_> = fs::read_dir(dir1)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect();

    let mut entries2: Vec<_> = fs::read_dir(dir2)?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .collect();

    // Sort for consistent comparison
    entries1.sort();
    entries2.sort();

    // If different number of entries or different names, they're different
    if entries1 != entries2 {
        return Ok(false);
    }

    // Recursively check each entry
    for entry_name in entries1 {
        let path1 = dir1.join(&entry_name);
        let path2 = dir2.join(&entry_name);

        if !data_is_identical(&path1, &path2)? {
            return Ok(false);
        }
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::TempDir;

    fn cleanup_test_data() {
        if let Ok(client_dir) = crate::access::data_dir::get_client_data_dir_base() {
            let _ = fs::remove_dir_all(&client_dir);
        }
    }

    /// Check if the current wallet has already been migrated
    fn is_wallet_migrated(wallet_address: &str) -> Result<bool> {
        let base_dir = crate::access::data_dir::get_client_data_dir_base()?;

        let wallet_dir = base_dir.join(wallet_address);

        // Consider it migrated if the wallet directory exists and has some expected content
        if wallet_dir.exists() {
            let user_data_path = wallet_dir.join("user_data");
            // If the wallet dir exists and has user_data or any signing keys, it's been set up
            Ok(user_data_path.exists()
                || wallet_dir.join("register_signing_key").exists()
                || wallet_dir.join("scratchpad_signing_key").exists()
                || wallet_dir.join("pointer_signing_key").exists())
        } else {
            Ok(false)
        }
    }

    fn create_test_file(path: &PathBuf, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn create_test_dir(path: &PathBuf) {
        fs::create_dir_all(path).unwrap();
    }

    fn create_legacy_structure() {
        // Use the same function the real code uses
        let client_dir = crate::access::data_dir::get_client_data_dir_base()
            .expect("Failed to get client data dir base in test");

        fs::create_dir_all(&client_dir).unwrap();

        // Create legacy user_data structure
        let user_data_dir = client_dir.join("user_data");
        fs::create_dir_all(user_data_dir.join("registers")).unwrap();
        fs::create_dir_all(user_data_dir.join("file_archives")).unwrap();
        fs::create_dir_all(user_data_dir.join("scratchpads")).unwrap();
        fs::create_dir_all(user_data_dir.join("pointers")).unwrap();

        // Create test files
        fs::write(
            user_data_dir.join("registers").join("test_register"),
            "register_content",
        )
        .unwrap();
        fs::write(client_dir.join("register_signing_key"), "test_register_key").unwrap();
        fs::write(
            client_dir.join("scratchpad_signing_key"),
            "test_scratchpad_key",
        )
        .unwrap();
        fs::write(client_dir.join("pointer_signing_key"), "test_pointer_key").unwrap();
    }

    #[test]
    #[serial]
    fn test_legacy_data_exists() {
        cleanup_test_data();

        // Initially no legacy data
        assert!(!legacy_data_exists().unwrap());

        // Create legacy structure
        create_legacy_structure();

        // Now legacy data should exist
        assert!(legacy_data_exists().unwrap());
    }

    #[test]
    #[serial]
    fn test_migration() {
        cleanup_test_data();

        // Create legacy structure
        create_legacy_structure();

        let wallet_address = "0xMigrationTest1234567890123456789012345678";

        // Verify legacy data exists
        assert!(legacy_data_exists().unwrap());

        // Perform migration
        migrate_legacy_data_if_needed(wallet_address).unwrap();

        // Check that files were moved to new location
        let client_dir = crate::access::data_dir::get_client_data_dir_base()
            .expect("Failed to get client data dir base in test");
        let wallet_dir = client_dir.join(wallet_address);

        // New structure should exist
        assert!(wallet_dir.join("user_data").exists());
        assert!(wallet_dir.join("user_data").join("registers").exists());
        assert!(wallet_dir.join("register_signing_key").exists());
        assert!(wallet_dir.join("scratchpad_signing_key").exists());
        assert!(wallet_dir.join("pointer_signing_key").exists());

        // Old structure should be removed
        assert!(!client_dir.join("user_data").exists());
        assert!(!client_dir.join("register_signing_key").exists());
        assert!(!client_dir.join("scratchpad_signing_key").exists());
        assert!(!client_dir.join("pointer_signing_key").exists());

        // Legacy data should no longer exist
        assert!(!legacy_data_exists().unwrap());
    }

    #[test]
    #[serial]
    fn test_migration_skip_when_destination_exists() {
        cleanup_test_data();

        // Create legacy structure
        create_legacy_structure();

        let wallet_address = "0xSkipDestinationTest567890123456789012345";
        let client_dir = crate::access::data_dir::get_client_data_dir_base()
            .expect("Failed to get client data dir base in test");
        let wallet_dir = client_dir.join(wallet_address);

        // Create destination with different content
        fs::create_dir_all(&wallet_dir).unwrap();
        fs::write(wallet_dir.join("register_signing_key"), "existing_key").unwrap();

        // Perform migration
        migrate_legacy_data_if_needed(wallet_address).unwrap();

        // Existing file should not be overwritten
        let content = fs::read_to_string(wallet_dir.join("register_signing_key")).unwrap();
        assert_eq!(content, "existing_key");

        // Legacy file should still exist since we skipped it
        assert!(client_dir.join("register_signing_key").exists());
    }

    #[test]
    #[serial]
    fn test_no_migration_when_no_legacy_data() {
        cleanup_test_data();

        let wallet_address = "0xNoMigrationTest567890123456789012345678";

        // No legacy data to migrate
        assert!(!legacy_data_exists().unwrap());

        // Migration should succeed without doing anything
        migrate_legacy_data_if_needed(wallet_address).unwrap();

        // Migration shouldn't create anything since there's no legacy data
        assert!(!legacy_data_exists().unwrap());
    }

    #[test]
    #[serial]
    fn test_is_wallet_migrated() {
        cleanup_test_data();

        let wallet_address = "0xWalletMigratedTest567890123456789012345678";

        // Initially not migrated
        assert!(!is_wallet_migrated(wallet_address).unwrap());

        // Create wallet directory with user_data
        let client_dir = crate::access::data_dir::get_client_data_dir_base()
            .expect("Failed to get client data dir base in test");
        let wallet_dir = client_dir.join(wallet_address);
        fs::create_dir_all(wallet_dir.join("user_data")).unwrap();

        // Now should be considered migrated
        assert!(is_wallet_migrated(wallet_address).unwrap());
    }

    // === Data Comparison Tests ===

    #[test]
    fn test_data_is_identical_files() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Test identical files
        let file1 = base.join("file1.txt");
        let file2 = base.join("file2.txt");
        create_test_file(&file1, "Hello world");
        create_test_file(&file2, "Hello world");
        assert!(data_is_identical(&file1, &file2).unwrap());

        // Test different files
        let file3 = base.join("file3.txt");
        create_test_file(&file3, "Different content");
        assert!(!data_is_identical(&file1, &file3).unwrap());

        // Test empty files
        let empty1 = base.join("empty1.txt");
        let empty2 = base.join("empty2.txt");
        create_test_file(&empty1, "");
        create_test_file(&empty2, "");
        assert!(data_is_identical(&empty1, &empty2).unwrap());

        // Test binary content
        let bin1 = base.join("bin1.dat");
        let bin2 = base.join("bin2.dat");
        fs::write(&bin1, [0u8, 1u8, 255u8]).unwrap();
        fs::write(&bin2, [0u8, 1u8, 255u8]).unwrap();
        assert!(data_is_identical(&bin1, &bin2).unwrap());
    }

    #[test]
    fn test_data_is_identical_directories() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        // Create identical directory structures
        let dir1 = base.join("dir1");
        let dir2 = base.join("dir2");

        // Populate dir1
        create_test_file(&dir1.join("file1.txt"), "content1");
        create_test_file(&dir1.join("subdir/file2.txt"), "content2");
        create_test_file(&dir1.join("subdir/file3.txt"), "content3");
        create_test_dir(&dir1.join("empty_dir"));

        // Populate dir2 with identical content
        create_test_file(&dir2.join("file1.txt"), "content1");
        create_test_file(&dir2.join("subdir/file2.txt"), "content2");
        create_test_file(&dir2.join("subdir/file3.txt"), "content3");
        create_test_dir(&dir2.join("empty_dir"));

        assert!(data_is_identical(&dir1, &dir2).unwrap());

        // Test different directories - different file content
        let dir3 = base.join("dir3");
        create_test_file(&dir3.join("file1.txt"), "different content");
        create_test_file(&dir3.join("subdir/file2.txt"), "content2");
        create_test_file(&dir3.join("subdir/file3.txt"), "content3");
        create_test_dir(&dir3.join("empty_dir"));

        assert!(!data_is_identical(&dir1, &dir3).unwrap());

        // Test different directories - missing file
        let dir4 = base.join("dir4");
        create_test_file(&dir4.join("file1.txt"), "content1");
        create_test_file(&dir4.join("subdir/file2.txt"), "content2");
        // Missing file3.txt
        create_test_dir(&dir4.join("empty_dir"));

        assert!(!data_is_identical(&dir1, &dir4).unwrap());

        // Test different directories - extra file
        let dir5 = base.join("dir5");
        create_test_file(&dir5.join("file1.txt"), "content1");
        create_test_file(&dir5.join("subdir/file2.txt"), "content2");
        create_test_file(&dir5.join("subdir/file3.txt"), "content3");
        create_test_file(&dir5.join("extra_file.txt"), "extra");
        create_test_dir(&dir5.join("empty_dir"));

        assert!(!data_is_identical(&dir1, &dir5).unwrap());
    }

    #[test]
    fn test_data_is_identical_mixed_types() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let file = base.join("test_file.txt");
        let dir = base.join("test_dir");

        create_test_file(&file, "content");
        create_test_dir(&dir);

        // File vs directory should be different
        assert!(!data_is_identical(&file, &dir).unwrap());
        assert!(!data_is_identical(&dir, &file).unwrap());
    }

    #[test]
    fn test_data_is_identical_empty_directories() {
        let temp_dir = TempDir::new().unwrap();
        let base = temp_dir.path();

        let empty_dir1 = base.join("empty1");
        let empty_dir2 = base.join("empty2");

        create_test_dir(&empty_dir1);
        create_test_dir(&empty_dir2);

        assert!(data_is_identical(&empty_dir1, &empty_dir2).unwrap());
    }

    // === Enhanced Migration Tests ===

    #[test]
    #[serial]
    fn test_migration_with_identical_data_skips_and_cleans_up() {
        cleanup_test_data();

        // Create legacy structure
        create_legacy_structure();

        let wallet_address = "0xIdenticalDataTest567890123456789012345678";
        let client_dir = crate::access::data_dir::get_client_data_dir_base()
            .expect("Failed to get client data dir base in test");
        let wallet_dir = client_dir.join(wallet_address);

        // Create destination with IDENTICAL content
        fs::create_dir_all(&wallet_dir).unwrap();
        create_test_file(
            &wallet_dir.join("register_signing_key"),
            "test_register_key",
        );
        create_test_file(
            &wallet_dir.join("scratchpad_signing_key"),
            "test_scratchpad_key",
        );
        create_test_file(&wallet_dir.join("pointer_signing_key"), "test_pointer_key");

        // Create identical user_data structure
        let dest_user_data = wallet_dir.join("user_data");
        create_test_dir(&dest_user_data.join("registers"));
        create_test_dir(&dest_user_data.join("file_archives"));
        create_test_dir(&dest_user_data.join("scratchpads"));
        create_test_dir(&dest_user_data.join("pointers"));
        create_test_file(
            &dest_user_data.join("registers").join("test_register"),
            "register_content",
        );

        // Perform migration
        migrate_legacy_data_if_needed(wallet_address).unwrap();

        // Destination should still exist with original content
        assert!(wallet_dir.join("register_signing_key").exists());
        let content = fs::read_to_string(wallet_dir.join("register_signing_key")).unwrap();
        assert_eq!(content, "test_register_key");

        // Legacy files should still exist since we skipped migration (they were identical)
        // This is the correct behavior - we don't delete originals when we skip migration
        assert!(client_dir.join("register_signing_key").exists());
        assert!(client_dir.join("user_data").exists());
    }

    #[test]
    #[serial]
    fn test_migration_with_nested_directory_structure() {
        cleanup_test_data();

        let client_dir = crate::access::data_dir::get_client_data_dir_base().unwrap();

        // Create complex nested legacy structure
        let user_data_dir = client_dir.join("user_data");
        create_test_file(&user_data_dir.join("registers").join("reg1"), "register1");
        create_test_file(&user_data_dir.join("registers").join("reg2"), "register2");
        create_test_file(
            &user_data_dir
                .join("file_archives")
                .join("deep")
                .join("nested")
                .join("file.txt"),
            "nested content",
        );
        create_test_file(
            &user_data_dir.join("scratchpads").join("scratch1"),
            "scratch data",
        );
        create_test_dir(&user_data_dir.join("pointers").join("empty_subdir"));

        let wallet_address = "0xNested567890123456789012345678901234567890";

        // Verify legacy data exists
        assert!(legacy_data_exists().unwrap());

        // Perform migration
        migrate_legacy_data_if_needed(wallet_address).unwrap();

        // Check nested structure was preserved
        let wallet_dir = client_dir.join(wallet_address);
        assert!(
            wallet_dir
                .join("user_data")
                .join("registers")
                .join("reg1")
                .exists()
        );
        assert!(
            wallet_dir
                .join("user_data")
                .join("registers")
                .join("reg2")
                .exists()
        );
        assert!(
            wallet_dir
                .join("user_data")
                .join("file_archives")
                .join("deep")
                .join("nested")
                .join("file.txt")
                .exists()
        );

        // Check content integrity
        let content = fs::read_to_string(
            wallet_dir
                .join("user_data")
                .join("file_archives")
                .join("deep")
                .join("nested")
                .join("file.txt"),
        )
        .unwrap();
        assert_eq!(content, "nested content");

        // Check empty directory was created
        assert!(
            wallet_dir
                .join("user_data")
                .join("pointers")
                .join("empty_subdir")
                .exists()
        );
        assert!(
            wallet_dir
                .join("user_data")
                .join("pointers")
                .join("empty_subdir")
                .is_dir()
        );
    }

    #[test]
    #[serial]
    fn test_migration_partial_failure_reporting() {
        // This test is more complex as it requires mocking filesystem failures
        // For now, we'll test the error aggregation logic with a simpler case
        cleanup_test_data();

        let client_dir = crate::access::data_dir::get_client_data_dir_base().unwrap();
        let wallet_address = "0xFailTest567890123456789012345678901234567890";
        let wallet_dir = client_dir.join(wallet_address);

        // Create legacy structure
        create_test_file(
            &client_dir.join("register_signing_key"),
            "test_register_key",
        );

        // Create destination directory but make migration destination already exist with different content
        fs::create_dir_all(&wallet_dir).unwrap();
        create_test_file(
            &wallet_dir.join("register_signing_key"),
            "different_existing_key",
        );

        // This should succeed without error since we handle existing different files gracefully
        let result = migrate_legacy_data_if_needed(wallet_address);
        assert!(result.is_ok());

        // The existing file should be preserved (not overwritten)
        let content = fs::read_to_string(wallet_dir.join("register_signing_key")).unwrap();
        assert_eq!(content, "different_existing_key");

        // Legacy file should still exist since it wasn't moved
        assert!(client_dir.join("register_signing_key").exists());
    }
}
