use std::path::{Path, PathBuf};

pub mod archive;
pub mod archive_public;
pub mod fs;
pub mod fs_public;

pub(crate) fn get_relative_file_path_from_abs_file_and_folder_path(
    abs_file_pah: &Path,
    abs_folder_path: &Path,
) -> PathBuf {
    // check if the dir is a file
    let is_file = abs_folder_path.is_file();

    // could also be the file name
    let dir_name = PathBuf::from(
        abs_folder_path
            .file_name()
            .expect("Failed to get file/dir name"),
    );

    if is_file {
        dir_name
    } else {
        let folder_prefix = abs_folder_path
            .parent()
            .unwrap_or(Path::new(""))
            .to_path_buf();
        abs_file_pah
            .strip_prefix(folder_prefix)
            .expect("Could not strip prefix path")
            .to_path_buf()
    }
}
