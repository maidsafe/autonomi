// Copyright 2023 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

use crate::subcommands::files::get_progress_bar;
use color_eyre::{eyre::bail, Result};
use rayon::prelude::{IntoParallelRefIterator, ParallelIterator};
use sn_client::Files;
use std::{
    collections::{btree_map, BTreeMap, BTreeSet},
    ffi::OsString,
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::Instant,
};
use walkdir::WalkDir;
use xor_name::XorName;

const CHUNK_ARTIFACTS_DIR: &str = "chunk_artifacts";
const UNPAID_DIR: &str = "unpaid";
const PAID_DIR: &str = "paid";
const METADATA_FILE: &str = "metadata";

// The unique hex encoded hash(path)
// This allows us to uniquely identify if a file has been chunked or not.
// An alternative to use instead of filename as it might not be unique
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
struct PathXorName(String);

impl PathXorName {
    fn new(path: &Path) -> PathXorName {
        // we just need an unique value per path, thus we don't have to mind between the
        // [u8]/[u16] differences
        let path_as_lossy_str = path.as_os_str().to_string_lossy();
        let path_xor = XorName::from_content(path_as_lossy_str.as_bytes());
        PathXorName(hex::encode(path_xor))
    }
}

/// Info about a file that has been chunked
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub(crate) struct ChunkedFile {
    pub file_name: OsString,
    pub file_xor_addr: XorName,
    pub chunks: BTreeSet<(XorName, PathBuf)>,
}

/// Manages the chunking process by resuming pre-chunked files and chunking any
/// file that has not been chunked yet.
#[derive(Debug, Clone, PartialEq, PartialOrd, Eq, Ord)]
pub(crate) struct ChunkManager {
    unpaid_dir: PathBuf,
    paid_dir: PathBuf,
    unpaid_chunks: BTreeMap<PathXorName, ChunkedFile>,
    paid_chunks: BTreeMap<PathXorName, ChunkedFile>,
    files_to_chunk: Vec<(OsString, PathXorName, PathBuf)>,
    verified_files: Vec<(OsString, XorName)>,
    resumed_unpaid_chunk_count: usize,
    resumed_paid_chunk_count: usize,
    resumed_files_count: usize,
}

impl ChunkManager {
    // Provide the root_dir. The function creates a sub-directory to store the SE chunks
    pub(crate) fn new(root_dir: &Path) -> Self {
        let artifacts_dir = root_dir.join(CHUNK_ARTIFACTS_DIR);
        Self {
            unpaid_dir: artifacts_dir.join(UNPAID_DIR),
            paid_dir: artifacts_dir.join(PAID_DIR),
            unpaid_chunks: Default::default(),
            paid_chunks: Default::default(),
            files_to_chunk: Default::default(),
            verified_files: Default::default(),
            resumed_unpaid_chunk_count: 0,
            resumed_paid_chunk_count: 0,
            resumed_files_count: 0,
        }
    }

    /// Chunk all the files in the provided `files_path`
    /// These are stored to the UNPAID_DIR
    pub(crate) fn chunk_path(&mut self, files_path: &Path) -> Result<()> {
        trace!("Starting to chunk {files_path:?} now.");
        let now = Instant::now();
        // clean up
        self.files_to_chunk = Default::default();
        self.paid_chunks = Default::default();
        self.unpaid_chunks = Default::default();
        self.verified_files = Default::default();
        self.resumed_unpaid_chunk_count = 0;
        self.resumed_paid_chunk_count = 0;
        self.resumed_files_count = 0;

        // collect the files to chunk
        WalkDir::new(files_path)
            .into_iter()
            .flatten()
            .for_each(|entry| {
                if entry.file_type().is_file() {
                    let path_xor = PathXorName::new(entry.path());
                    info!(
                        "Added file {:?} with path_xor: {path_xor:?} to be chunked/resumed",
                        entry.path()
                    );
                    self.files_to_chunk.push((
                        entry.file_name().to_owned(),
                        path_xor,
                        entry.into_path(),
                    ));
                }
            });
        let total_files = self.files_to_chunk.len();

        // resume the both unpaid and paid chunks
        self.resume_path();

        // note the number of chunks that we've resumed
        self.resumed_unpaid_chunk_count = self
            .unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .count();
        self.resumed_paid_chunk_count = self
            .paid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .count();
        // note the number of files that we've resumed
        self.resumed_files_count = self
            .unpaid_chunks
            .keys()
            .chain(self.paid_chunks.keys())
            .collect::<BTreeSet<_>>()
            .len();

        // Filter out files_to_chunk; Any PathXorName in unpaid/paid is considered to be resumed.
        {
            let path_xors = self
                .unpaid_chunks
                .keys()
                .chain(self.paid_chunks.keys())
                .collect::<BTreeSet<_>>();
            self.files_to_chunk
                .retain(|(_, path_xor, _)| !path_xors.contains(path_xor));
        }

        // Get the list of verified files
        {
            let verified_files = self
                .paid_chunks
                .iter()
                .filter_map(|(path_xor, chunked_file)| {
                    // Iff paid chunks is empty but unpaid is not, then we don't add that to verified list.
                    // As some files are still unpaid.
                    if let Some(unpaid_chunked_file) = self.unpaid_chunks.get(path_xor) {
                        if !unpaid_chunked_file.chunks.is_empty() {
                            return None;
                        }
                    }
                    if chunked_file.chunks.is_empty() {
                        Some((chunked_file.file_name.clone(), chunked_file.file_xor_addr))
                    } else {
                        None
                    }
                });

            self.verified_files.extend(verified_files);
        }

        // Return early if no more files to chunk
        if self.files_to_chunk.is_empty() {
            debug!(
                "All files_to_chunk ({total_files:?}) were resumed. Returning the resumed chunks.",
            );
            debug!("It took {:?} to resume all the files", now.elapsed());
            return Ok(());
        }

        let progress_bar = get_progress_bar(total_files as u64)?;
        progress_bar.println(format!("Chunking {total_files} files..."));

        let unpaid_dir = &self.unpaid_dir.clone();
        let chunked_files = self.files_to_chunk
            .par_iter()
            .filter_map(|(original_file_name, path_xor, path)| {
                let file_chunks_dir = {
                    let file_chunks_dir = unpaid_dir.join(&path_xor.0);
                    match fs::create_dir_all(&file_chunks_dir) {
                        Ok(_) => file_chunks_dir,
                        Err(err) => {
                            println!("Failed to create temp folder {file_chunks_dir:?} for SE chunks with error {err:?}!");
                            error!("Failed to create temp folder {file_chunks_dir:?} for SE chunks with error {err:?}!");
                            // use the chunk_artifacts_dir directly; This should not result in any
                            // undefined behaviour. The resume operation will be disabled if we don't
                            // use the `path_xor` dir.
                            // TODO: maybe error out if we get any fs errors.
                            unpaid_dir.clone()
                        }
                    }
                };

                match Files::chunk_file(path, &file_chunks_dir) {
                    Ok((file_xor_addr, size, chunks)) => {
                        progress_bar.clone().inc(1);
                        debug!("Chunked {original_file_name:?} with {path_xor:?} into file's XorName: {file_xor_addr:?} of size {size}, and chunks len: {}", chunks.len());

                        let chunked_file = ChunkedFile {
                            file_xor_addr,
                            file_name: original_file_name.clone(),
                            chunks: chunks.into_iter().collect()
                        };
                        Some((path_xor.clone(), chunked_file))
                    }
                    Err(err) => {
                        println!("Skipping file {path:?}/{path_xor:?} as it could not be chunked: {err:?}");
                        error!("Skipping file {path:?}/{path_xor:?} as it could not be chunked: {err:?}");
                        None
                    }
                }
            })
            .collect::<BTreeMap<_, _>>();
        debug!(
            "Out of total files_to_chunk {total_files}, we have resumed {} files and chunked {} files",
            self.resumed_files_count,
            chunked_files.len()
        );

        if chunked_files.is_empty() && self.paid_chunks.is_empty() && self.unpaid_chunks.is_empty()
        {
            bail!(
                "The provided path does not contain any file. Please check your path!\nExiting..."
            );
        }

        // write metadata
        let _ = chunked_files
            .par_iter()
            .filter_map(|(path_xor, chunked_file)| {
                let metadata_path = unpaid_dir.join(&path_xor.0).join(METADATA_FILE);
                let metadata = bincode::serialize(&chunked_file.file_xor_addr)
                    .map_err(|_| error!("Failed to serialize file_xor_addr for writing metadata"))
                    .ok()?;
                let mut metadata_file = File::create(&metadata_path)
                    .map_err(|_| {
                        error!("Failed to create metadata_path {metadata_path:?} for {path_xor:?}")
                    })
                    .ok()?;
                metadata_file
                    .write_all(&metadata)
                    .map_err(|_| {
                        error!("Failed to write metadata to {metadata_path:?} for {path_xor:?}")
                    })
                    .ok()?;
                debug!("Wrote metadata for {path_xor:?}");
                Some(())
            })
            .count();

        progress_bar.finish_and_clear();
        debug!(
            "It took {:?} to chunk {} files",
            now.elapsed(),
            self.files_to_chunk.len()
        );
        self.unpaid_chunks.extend(chunked_files);

        Ok(())
    }

    // Try to resume all the unpaid and paid chunks
    // Return the set of chunked_files if that we were able to resume.
    fn resume_path(&mut self) {
        let unpaid_dir = self.unpaid_dir.clone();
        let unpaid = self
            .files_to_chunk
            .par_iter()
            .filter_map(|(original_file_name, path_xor, _)| {
                // if this folder exists, and if we find chunks under this, we upload them.
                let file_chunks_dir = unpaid_dir.join(&path_xor.0);
                if !file_chunks_dir.exists() {
                    return None;
                }
                Self::read_file_chunks_dir(
                    file_chunks_dir,
                    path_xor.clone(),
                    original_file_name.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        self.unpaid_chunks.extend(unpaid);

        let paid = self.paid_dir.clone();
        let paid = self
            .files_to_chunk
            .par_iter()
            .filter_map(|(original_file_name, path_xor, _)| {
                // if this folder exists, and if we find chunks under this, we upload them.
                let file_chunks_dir = paid.join(&path_xor.0);
                if !file_chunks_dir.exists() {
                    return None;
                }
                Self::read_file_chunks_dir(
                    file_chunks_dir,
                    path_xor.clone(),
                    original_file_name.clone(),
                )
            })
            .collect::<BTreeMap<_, _>>();

        self.paid_chunks.extend(paid);
    }

    /// Get all the unpaid chunk name and their path
    pub(crate) fn get_unpaid_chunks(&self) -> Vec<(XorName, PathBuf)> {
        self.unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .cloned()
            .collect()
    }

    /// Get all the paid chunk name and their path
    pub(crate) fn get_paid_chunks(&self) -> Vec<(XorName, PathBuf)> {
        self.paid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .cloned()
            .collect()
    }

    pub(crate) fn is_unpaid_chunks_empty(&self) -> bool {
        self.unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .next()
            .is_none()
    }

    pub(crate) fn is_paid_chunks_empty(&self) -> bool {
        self.paid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .next()
            .is_none()
    }

    /// Mark all the unpaid chunks as paid and move them from the UNPAID_DIR to PAID_DIR
    /// Also removes the dir from UNPAID_DIR
    pub(crate) fn _mark_paid_all(&mut self) {
        let all_chunks = self
            .unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .map(|(chunk, _)| *chunk)
            .collect::<Vec<_>>();
        self.mark_paid(all_chunks.into_iter());
    }

    /// Mark a set of chunks as paid and move them from the UNPAID_DIR to PAID_DIR
    /// If the entire file is paid for, then remove the entire dir.
    pub(crate) fn mark_paid(&mut self, chunks: impl Iterator<Item = XorName>) {
        let set_of_paid_chunk = chunks.collect::<BTreeSet<_>>();
        trace!("marking as paid: {set_of_paid_chunk:?}");
        let paid_dir = self.paid_dir.clone();
        let unpaid_dir = self.unpaid_dir.clone();

        // Get all the chunks from unpaid. If they're part of the set of paid_chunks,
        // move them to the PAID_DIR and take a note of their new paths
        let new_chunk_paths = self
            .unpaid_chunks
            .par_iter()
            .flat_map(|(path_xor, chunked_file)| {
                // Make sure the PAID_DIR/xor_path & PAID_DIR/xor_path/metadata exists.
                // This only need to be undertaken at per file level.
                let new_path = paid_dir.join(path_xor.0.clone());
                if !new_path.exists() {
                    if let Err(err) = fs::create_dir_all(&new_path) {
                        error!("Failed to create dir inside PAID_DIR {new_path:?}: {err:?}");
                    }
                    let new_metadata = new_path.join(METADATA_FILE);
                    let old_metadata = unpaid_dir.join(path_xor.0.clone()).join(METADATA_FILE);

                    if let Err(err) = fs::copy(&old_metadata, &new_metadata) {
                       error!("Failed to copy metadata file from {old_metadata:?} to {new_metadata:?}: {err:?}") 
                    }
                }

                chunked_file
                    .chunks
                    .par_iter()
                    .map(|chunk| (path_xor.clone(), chunk))
            })
            .filter_map(|(path_xor, (chunk_xor, chunk_path))| {
                if set_of_paid_chunk.contains(chunk_xor) {
                    let new_path = paid_dir.join(path_xor.0).join(Self::hex_encode_xorname(*chunk_xor));

                    if let Err(err) = fs::rename(chunk_path, &new_path) {
                        error!("Failed to move SE chunk from {chunk_path:?} to {new_path:?}: {err:?}");
                        None
                    } else {
                        Some((*chunk_xor, new_path))
                    }
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_,_>>();

        let mut entire_file_is_paid = BTreeSet::new();
        let mut move_to_paid = BTreeMap::new();
        // remove the paid chunks from unpaid_chunks::ChunkedFile::chunks
        self.unpaid_chunks
            .iter_mut()
            .for_each(|(path_xor, chunked_file)| {
                let mut all_moved_to_new_path = true;
                chunked_file.chunks.retain(|(chunk_xor, chunk_path)| {
                    if set_of_paid_chunk.contains(chunk_xor) {
                        move_to_paid.insert(
                            *chunk_xor,
                            (
                                chunk_path.clone(),
                                path_xor,
                                chunked_file.file_name.clone(),
                                chunked_file.file_xor_addr,
                            ),
                        );
                        if !new_chunk_paths.contains_key(chunk_xor) {
                            all_moved_to_new_path = false;
                        }
                        // don't retain it
                        false
                    } else {
                        true
                    }
                });

                // Whenever there is a chunk failed to be moved into new path,
                // the parent `file folder` shall not be removed.
                if chunked_file.chunks.is_empty() && all_moved_to_new_path {
                    entire_file_is_paid.insert(path_xor.clone());
                }
            });

        // for each paid entry, insert them into the paid_chunks field with their new paths
        for (chunk_xor, (chunk_path, path_xor, file_name, file_xor_addr)) in move_to_paid {
            // change to PAID_DIR
            let chunk_path = if let Some(new_path) = new_chunk_paths.get(&chunk_xor) {
                new_path.clone()
            } else {
                error!("Could not retrieve the PAID chunk path of {chunk_xor:?}. Something went wrong. ");
                // using the old one; assuming that it might be there?
                chunk_path
            };

            match self.paid_chunks.entry(path_xor.clone()) {
                btree_map::Entry::Vacant(entry) => {
                    let mut chunks = BTreeSet::new();
                    chunks.insert((chunk_xor, chunk_path));
                    let _ = entry.insert(ChunkedFile {
                        file_name: file_name.clone(),
                        file_xor_addr,
                        chunks,
                    });
                }
                btree_map::Entry::Occupied(mut entry) => {
                    entry.get_mut().chunks.insert((chunk_xor, chunk_path));
                }
            }
        }

        // The dir can be removed entirely if done
        // Also remove the entry from struct
        for path_xor in &entire_file_is_paid {
            let _ = self.unpaid_chunks.remove(path_xor);
            let path = self.unpaid_dir.join(&path_xor.0);
            debug!("Removing the entire unpaid dir {path:?} dir as it is fully paid");
            if let Err(err) = fs::remove_dir_all(&path) {
                error!("Error while removing {path:?} err: {err:?}");
            }
        }
    }

    /// Mark all the paid chunks as verified and remove them from PAID_DIR
    /// Retains the folder and metadata file
    pub(crate) fn _mark_verified_all(&mut self) {
        let all_chunks = self
            .paid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .map(|(chunk, _)| *chunk)
            .collect::<Vec<_>>();
        self.mark_verified(all_chunks.into_iter());
    }

    /// Mark a set of chunks as verified and remove them from PAID_DIR
    /// If the entire file is verified, keep the folder and metadata file
    pub(crate) fn mark_verified(&mut self, chunks: impl Iterator<Item = XorName>) {
        let set_of_verified_chunk = chunks.collect::<BTreeSet<_>>();
        trace!("marking as verified: {set_of_verified_chunk:?}");

        // make sure they have already been marked as paid, if not, mark them as paid.
        let still_unpaid = self
            .unpaid_chunks
            .iter()
            .flat_map(|(_, chunked_file)| &chunked_file.chunks)
            .filter_map(|(chunk_xor, _)| {
                if set_of_verified_chunk.contains(chunk_xor) {
                    Some(*chunk_xor)
                } else {
                    None
                }
            })
            .collect::<BTreeSet<_>>();
        self.mark_paid(still_unpaid.into_iter());

        // remove those files
        let _ = self
            .paid_chunks
            .par_iter()
            .flat_map(|(_, chunked_file)| &chunked_file.chunks)
            .filter_map(|(chunk_xor, chunk_path)| {
                if set_of_verified_chunk.contains(chunk_xor) {
                    debug!("removing {chunk_xor:?} at {chunk_path:?} as it is marked as verified");
                    fs::remove_file(chunk_path)
                        .map_err(|_err| {
                            error!("Failed to remove SE chunk {chunk_xor} from {chunk_path:?}");
                        })
                        .ok()?;
                }
                Some(())
            })
            .count();

        let mut entire_file_is_done = BTreeSet::new();
        // remove the entries from the struct
        self.paid_chunks
            .iter_mut()
            .for_each(|(path_xor, chunked_file)| {
                chunked_file
                    .chunks
                    // if chunk is part of completed_chunks, return false to remove it
                    .retain(|(chunk_xor, _)| !set_of_verified_chunk.contains(chunk_xor));
                if chunked_file.chunks.is_empty() {
                    // if still part of unpaid, then don't remove it
                    if let Some(unpaid_chunked_file) = self.unpaid_chunks.get(path_xor) {
                        if unpaid_chunked_file.chunks.is_empty() {
                            entire_file_is_done.insert(path_xor.clone());
                        }
                    } else {
                        entire_file_is_done.insert(path_xor.clone());
                    }
                }
            });

        for path_xor in &entire_file_is_done {
            if let Some(chunked_file) = self.paid_chunks.remove(path_xor) {
                trace!("removed {path_xor:?} from paid_chunks");
                self.verified_files
                    .push((chunked_file.file_name, chunked_file.file_xor_addr));
            }
        }
    }

    /// Return the filename and the file's Xor address if all their chunks has been marked as
    /// verified
    pub(crate) fn verified_files(&self) -> &Vec<(OsString, XorName)> {
        &self.verified_files
    }

    // Try to read the chunks from `file_chunks_dir`
    // Returns the ChunkedFile if the metadata file exists
    // file_chunks_dir: artifacts_dir/paid_or_unpaid/path_xor
    // path_xor: Used during logging and is returned
    // original_file_name: Used to create ChunkedFile
    fn read_file_chunks_dir(
        file_chunks_dir: PathBuf,
        path_xor: PathXorName,
        original_file_name: OsString,
    ) -> Option<(PathXorName, ChunkedFile)> {
        let mut file_xor_addr: Option<XorName> = None;
        debug!("Trying to resume {path_xor:?} as the file_chunks_dir exists");

        let chunks = WalkDir::new(file_chunks_dir)
            .into_iter()
            .flatten()
            .filter_map(|entry| {
                if !entry.file_type().is_file() {
                    return None;
                }
                if entry.file_name() == METADATA_FILE {
                    if let Some(metadata) = Self::try_read_metadata(entry.path()) {
                        file_xor_addr = Some(metadata);
                        debug!("Obtained metadata for {path_xor:?}");
                    } else {
                        error!("Could not read metadata for {path_xor:?}");
                    }
                    // not a chunk, so don't return
                    return None;
                }
                // try to get the chunk's xorname from its filename
                if let Some(file_name) = entry.file_name().to_str() {
                    Self::hex_decode_xorname(file_name)
                        .map(|chunk_xorname| (chunk_xorname, entry.into_path()))
                } else {
                    error!(
                        "Failed to convert OsString to str for {:?}",
                        entry.file_name()
                    );
                    None
                }
            })
            .collect::<BTreeSet<_>>();

        match file_xor_addr {
            Some(file_xor_addr) => {
                debug!("Resuming {} chunks for file {original_file_name:?} and with file_xor_addr {file_xor_addr:?}/{path_xor:?}", chunks.len());

                Some((
                    path_xor.clone(),
                    ChunkedFile {
                        file_name: original_file_name.clone(),
                        file_xor_addr,
                        chunks,
                    },
                ))
            }
            None => {
                error!("Metadata file was not present for {path_xor:?}");
                // metadata file was not present/was not read
                None
            }
        }
    }

    // Try to read the metadata file
    fn try_read_metadata(path: &Path) -> Option<XorName> {
        let metadata = fs::read(path)
            .map_err(|err| error!("Failed to read metadata with err {err:?}"))
            .ok()?;
        let metadata: XorName = bincode::deserialize(&metadata)
            .map_err(|err| error!("Failed to deserialize metadata with err {err:?}"))
            .ok()?;
        Some(metadata)
    }

    // Decode the hex encoded xorname
    fn hex_decode_xorname(string: &str) -> Option<XorName> {
        let hex_decoded = hex::decode(string)
            .map_err(|err| error!("Failed to decode {string} into bytes with err {err:?}"))
            .ok()?;
        let decoded_xorname: [u8; xor_name::XOR_NAME_LEN] = hex_decoded
            .try_into()
            .map_err(|_| error!("Failed to convert hex_decoded xorname into an [u8; 32]"))
            .ok()?;
        Some(XorName(decoded_xorname))
    }

    fn hex_encode_xorname(xorname: XorName) -> String {
        hex::encode(xorname)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use color_eyre::{eyre::eyre, Result};
    use rand::{thread_rng, Rng};
    use rayon::prelude::IntoParallelIterator;
    use sn_logging::LogBuilder;
    use sn_protocol::test_utils::assert_list_eq;
    use tempfile::TempDir;

    #[test]
    fn chunked_files_should_be_written_to_unpaid_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;
        let unpaid_dir = manager.unpaid_dir.clone();
        let _ = create_random_files(&random_files_dir, 1, 1)?;
        manager.chunk_path(&random_files_dir)?;

        let chunks = manager.get_unpaid_chunks();
        // 1. 1mb file produces 4 chunks
        assert_eq!(chunks.len(), 4);

        // 2. make sure we have 1 folder == 1 file
        let n_folders = WalkDir::new(&unpaid_dir)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_dir() && entry.path() != unpaid_dir)
            .count();
        assert_eq!(n_folders, 1);

        // 3. make sure we have the 1 file per chunk + 1 metadata file
        let n_files = WalkDir::new(&unpaid_dir)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_file())
            .count();
        assert_eq!(n_files, chunks.len() + 1);

        // 4. make sure metadata file holds the correct file_xor_addr
        let mut file_xor_addr_from_metadata = None;
        for entry in WalkDir::new(&unpaid_dir).into_iter().flatten() {
            if entry.file_type().is_file() && entry.file_name() == METADATA_FILE {
                file_xor_addr_from_metadata = ChunkManager::try_read_metadata(entry.path());
            }
        }
        let file_xor_addr_from_metadata =
            file_xor_addr_from_metadata.expect("The metadata file should be present");
        let file_xor_addr = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("1 file should be present")
            .file_xor_addr;
        assert_eq!(file_xor_addr_from_metadata, file_xor_addr);

        // 5. make sure the chunked file's name is the XorName of that chunk
        let chunk_xornames = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("We must have 1 file here")
            .chunks
            .iter()
            .map(|(xor_name, _)| *xor_name)
            .collect::<BTreeSet<_>>();
        for entry in WalkDir::new(&unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                assert!(chunk_xornames.contains(&chunk_xorname_from_filename));
            }
        }

        Ok(())
    }

    #[test]
    // 1. UNPAID: simple case
    fn chunks_should_be_moved_to_paid_dir_if_marked_as_paid() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;

        let random_file_name = create_random_files(&random_files_dir, 1, 1)?
            .remove(0)
            .file_name()
            .unwrap()
            .to_owned();
        manager.chunk_path(&random_files_dir)?;

        assert_eq!(manager.unpaid_chunks.len(), 1);
        assert!(manager.paid_chunks.is_empty());

        // 1. check the files initially
        let path_xor = manager.unpaid_chunks.keys().next().unwrap().clone();
        let file_chunks_dir = manager.unpaid_dir.join(&path_xor.0);
        let chunk_entry = ChunkManager::read_file_chunks_dir(
            file_chunks_dir,
            path_xor.clone(),
            random_file_name.clone(),
        )
        .expect("Chunk should be present");
        assert_eq!(
            manager.unpaid_chunks.iter().next().unwrap(),
            (&chunk_entry.0, &chunk_entry.1)
        );

        // 2. check if a random chunk's path is set correctly
        let (unpaid_chunk, unpaid_chunk_path) = manager
            .unpaid_chunks
            .values()
            .next()
            .unwrap()
            .chunks
            .first()
            .expect("Must contain 1 chunk")
            .clone();
        let mut unpaid_chunk_path_clone = unpaid_chunk_path.clone();
        unpaid_chunk_path_clone.pop();
        unpaid_chunk_path_clone.pop();
        assert_eq!(unpaid_chunk_path_clone, manager.unpaid_dir);

        // mark a random chunk as paid
        manager.mark_paid(vec![unpaid_chunk].into_iter());

        // 3. check the structs
        assert_eq!(manager.unpaid_chunks.len(), 1);
        assert!(!manager
            .unpaid_chunks
            .values()
            .next()
            .unwrap()
            .chunks
            .contains(&(unpaid_chunk, unpaid_chunk_path)));
        assert_eq!(manager.paid_chunks.len(), 1);
        assert_eq!(manager.paid_chunks.values().next().unwrap().chunks.len(), 1);

        // 4. check the files inside unpaid dir.
        let file_chunks_dir = manager.unpaid_dir.join(&path_xor.0);
        let chunk_entry = ChunkManager::read_file_chunks_dir(
            file_chunks_dir,
            path_xor.clone(),
            random_file_name.clone(),
        )
        .expect("Chunk should be present");
        assert_eq!(
            manager.unpaid_chunks.iter().next().unwrap(),
            (&chunk_entry.0, &chunk_entry.1)
        );

        // 5. check the files inside paid dir
        let file_chunks_dir = manager.paid_dir.join(&path_xor.0);
        let chunk_entry =
            ChunkManager::read_file_chunks_dir(file_chunks_dir, path_xor, random_file_name)
                .expect("Chunk should be present");
        assert_eq!(
            manager.paid_chunks.iter().next().unwrap(),
            (&chunk_entry.0, &chunk_entry.1)
        );

        // 6. check if the random chunks' path is set correctly to PAID_DIR now
        let (paid_chunk, mut paid_chunk_path) = manager
            .paid_chunks
            .values()
            .next()
            .unwrap()
            .chunks
            .first()
            .expect("Must contain 1 chunk")
            .clone();
        paid_chunk_path.pop();
        paid_chunk_path.pop();
        assert_eq!(paid_chunk_path, manager.paid_dir);
        assert_eq!(unpaid_chunk, paid_chunk);

        Ok(())
    }

    #[test]
    // 2. UNPAID: if all the chunks are moved, the dir should be removed.
    fn marking_all_chunks_as_paid_should_remove_the_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;

        let n_folders = WalkDir::new(&manager.unpaid_dir)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_dir() && entry.path() != manager.unpaid_dir)
            .count();
        assert_eq!(n_folders, 5);

        manager._mark_paid_all();

        let n_folders = WalkDir::new(&manager.unpaid_dir)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_dir() && entry.path() != manager.unpaid_dir)
            .count();
        assert_eq!(n_folders, 0);

        Ok(())
    }

    #[test]
    // 1. PAID: simple case
    fn chunks_should_be_removed_from_paid_dir_if_marked_as_verified() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 1, 1)?;
        manager.chunk_path(&random_files_dir)?;

        let path_xor = manager.unpaid_chunks.keys().next().unwrap().clone();
        let chunked_file = manager.unpaid_chunks.values().next().unwrap().clone();
        let file_xor_addr = chunked_file.file_xor_addr;
        let (chunk, _) = chunked_file
            .chunks
            .first()
            .expect("Must contain 1 chunk")
            .clone();
        manager.mark_paid(vec![chunk].into_iter());
        manager.mark_verified(vec![chunk].into_iter());

        // 1. chunk should be removed from paid struct
        assert_eq!(
            manager
                .paid_chunks
                .values()
                .next()
                .expect("Since unpaid still exists, it should be present")
                .chunks
                .len(),
            0
        );

        // 2. the folder should exists, but chunk removed
        let file_chunks_dir = manager.paid_dir.join(&path_xor.0);
        let (path_xor_from_dir, chunked_file_from_dir) = ChunkManager::read_file_chunks_dir(
            file_chunks_dir,
            path_xor.clone(),
            chunked_file.file_name.to_owned(),
        )
        .expect("Folder and metadata should be present");
        assert_eq!(chunked_file_from_dir.chunks.len(), 0);
        assert_eq!(chunked_file_from_dir.file_xor_addr, file_xor_addr);
        assert_eq!(path_xor_from_dir, path_xor);

        // 2. file should not be marked as verified
        // since unpaid exists
        assert!(manager.verified_files.is_empty());

        Ok(())
    }

    #[test]
    // 2. PAID: if all the chunks are verified, the paid_dirs should exists
    fn marking_all_chunks_as_verified_should_not_remove_the_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;
        // cloned after chunking
        let manager_clone = manager.clone();

        let n_folders = WalkDir::new(&manager.unpaid_dir)
            .into_iter()
            .flatten()
            .filter(|entry| entry.file_type().is_dir() && entry.path() != manager.unpaid_dir)
            .count();
        assert_eq!(n_folders, 5);

        manager._mark_paid_all();
        manager._mark_verified_all();

        // all 5 files should be marked as verified
        assert_eq!(manager.verified_files.len(), 5);

        // all 5 folders should exist
        for (path_xor, chunked_file) in manager_clone.unpaid_chunks.iter() {
            let file_chunks_dir = manager_clone.paid_dir.join(path_xor.0.clone());
            let (path_xor_from_dir, chunked_file_from_dir) = ChunkManager::read_file_chunks_dir(
                file_chunks_dir,
                path_xor.clone(),
                chunked_file.file_name.to_owned(),
            )
            .expect("Folder and metadata should be present");
            assert_eq!(chunked_file_from_dir.chunks.len(), 0);
            assert_eq!(
                chunked_file_from_dir.file_xor_addr,
                chunked_file.file_xor_addr
            );
            assert_eq!(&path_xor_from_dir, path_xor);
        }

        Ok(())
    }

    #[test]
    //3. PAID: mark all as verified before marking as paid should have call mark_paid internally
    fn marking_as_verified_before_paid_should_not_result_in_undefined_behaviour() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 1, 1)?;
        manager.chunk_path(&random_files_dir)?;

        let path_xor = manager.unpaid_chunks.keys().next().unwrap().clone();
        let chunked_file = manager.unpaid_chunks.values().next().unwrap().clone();
        let file_xor_addr = chunked_file.file_xor_addr;

        // marking all as verified should not do anything
        let manager_clone = manager.clone();
        manager._mark_verified_all();

        // 1. _mark_verified_all() directly does nothing
        assert_eq!(manager, manager_clone);

        // get all the chunks and then mark as verified. This should trigger mark_paid
        manager.mark_verified(chunked_file.chunks.clone().into_iter().map(|(c, _)| c));

        // 2. chunk should be removed from paid and unpaid struct
        assert!(manager.unpaid_chunks.keys().next().is_none());
        assert!(manager.paid_chunks.keys().next().is_none());

        // 3. the folder should exists, but chunk removed
        let file_chunks_dir = manager.paid_dir.join(&path_xor.0);
        let (path_xor_from_dir, chunked_file_from_dir) = ChunkManager::read_file_chunks_dir(
            file_chunks_dir,
            path_xor.clone(),
            chunked_file.file_name.to_owned(),
        )
        .expect("Folder and metadata should be present");
        assert_eq!(chunked_file_from_dir.chunks.len(), 0);
        assert_eq!(chunked_file_from_dir.file_xor_addr, file_xor_addr);
        assert_eq!(path_xor_from_dir, path_xor);

        // 2. should be added to verified files
        assert_eq!(manager.verified_files.len(), 1);

        Ok(())
    }

    #[test]
    // RESUME: 1. mark none as paid -> try resume all
    fn mark_none_and_resume() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, root_dir, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;

        let mut new_manager = ChunkManager::new(&root_dir);
        new_manager.chunk_path(&random_files_dir)?;
        // 1. make sure we don't have any paid chunks
        assert_eq!(new_manager.paid_chunks.len(), 0);

        // 2. make sure the chunk counts match
        let original_unpaid_count = manager
            .unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .count();
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);
        assert_eq!(manager.resumed_paid_chunk_count, 0);
        assert_eq!(
            new_manager.resumed_unpaid_chunk_count,
            original_unpaid_count
        );
        assert_eq!(new_manager.resumed_paid_chunk_count, 0);

        // 3. assert the two managers
        assert_eq!(manager.unpaid_chunks, new_manager.unpaid_chunks);
        assert_eq!(manager.paid_chunks, new_manager.paid_chunks);
        assert_eq!(manager.verified_files, new_manager.verified_files);

        Ok(())
    }

    #[test]
    // RESUME: 2 .mark one as paid -> resume, unpaid and paid should be present
    // mark the above as verified -> resume, still should have the unpaid and should not be added to verified file;
    fn mark_one_chunk_as_paid_and_resume() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, root_dir, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;

        let original_unpaid_count = manager
            .unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .count();

        // mark a chunk as completed
        let removed_chunk = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("Atleast 1 file should be present")
            .chunks
            .iter()
            .next()
            .expect("Chunk should be present")
            .0;
        manager.mark_paid([removed_chunk].into_iter());
        let mut new_manager = ChunkManager::new(&root_dir);
        new_manager.chunk_path(&random_files_dir)?;

        // 1. we should have 1 paid chunk and (original_unpaid_count-1) unpaid chunks
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);
        assert_eq!(manager.resumed_paid_chunk_count, 0);
        assert_eq!(
            new_manager.resumed_unpaid_chunk_count,
            original_unpaid_count - 1
        );
        assert_eq!(new_manager.resumed_paid_chunk_count, 1);
        // also check the structs
        assert_eq!(
            new_manager
                .unpaid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            original_unpaid_count - 1
        );
        assert_eq!(
            new_manager
                .paid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            1
        );

        // 2. files should not be added to verified
        assert_eq!(new_manager.verified_files.len(), 0);

        // mark the same as verified
        new_manager.mark_verified([removed_chunk].into_iter());
        let mut new_manager = ChunkManager::new(&root_dir); // create a new manager
        new_manager.chunk_path(&random_files_dir)?;

        // 3. we should have 0 paid chunks and (original_unpaid_count-1) unpaid chunks
        assert_eq!(
            new_manager.resumed_unpaid_chunk_count,
            original_unpaid_count - 1
        );
        assert_eq!(new_manager.resumed_paid_chunk_count, 0);
        assert_eq!(
            new_manager
                .unpaid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            original_unpaid_count - 1
        );
        assert_eq!(
            new_manager
                .paid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            0
        );

        // 3. files should not be added to verified since entries still exists; we just verified 1 chunk
        assert_eq!(new_manager.verified_files.len(), 0);

        Ok(())
    }

    #[test]
    // RESUME: 3. mark the rest as paid -> resume, nothing in unpaid (no entry), paid should have entry;
    // Also, mark one as verified -> resume, should not be added to verified files
    fn mark_all_as_paid_and_resume() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, root_dir, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;

        let original_unpaid_count = manager
            .unpaid_chunks
            .values()
            .flat_map(|chunked_file| &chunked_file.chunks)
            .count();

        // mark all as paid
        manager._mark_paid_all();
        let mut new_manager = ChunkManager::new(&root_dir);
        new_manager.chunk_path(&random_files_dir)?;

        // 1. we should have original_unpaid_count paid chunks and 0 unpaid chunks
        assert_eq!(new_manager.resumed_unpaid_chunk_count, 0,);
        assert_eq!(new_manager.resumed_paid_chunk_count, original_unpaid_count);
        assert_eq!(new_manager.unpaid_chunks.len(), 0); // no unpaid entry
        assert_eq!(
            new_manager
                .paid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            original_unpaid_count,
        );

        // 2. no verified file
        assert_eq!(new_manager.verified_files.len(), 0);

        // mark the same as verified
        let removed_chunk = new_manager
            .paid_chunks
            .values()
            .next()
            .expect("Atleast 1 file should be present")
            .chunks
            .iter()
            .next()
            .expect("Chunk should be present")
            .0;
        new_manager.mark_verified([removed_chunk].into_iter());
        let mut new_manager = ChunkManager::new(&root_dir);
        new_manager.chunk_path(&random_files_dir)?;

        // 3. we should have (original_unpaid_count - 1) paid chunks and 0 unpaid chunks
        assert_eq!(new_manager.resumed_unpaid_chunk_count, 0,);
        assert_eq!(
            new_manager.resumed_paid_chunk_count,
            original_unpaid_count - 1
        );
        assert_eq!(new_manager.unpaid_chunks.len(), 0); // no unpaid entry
        assert_eq!(
            new_manager
                .paid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            original_unpaid_count - 1,
        );

        // 4. no verified file
        assert_eq!(new_manager.verified_files.len(), 0);

        Ok(())
    }

    #[test]
    //4. RESUME: mark all as verified -> resume, we should have paid entries. File should be added to verified_files
    fn mark_all_as_verified_and_resume() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, root_dir, random_files_dir) = init_manager()?;

        let _ = create_random_files(&random_files_dir, 5, 5)?;
        manager.chunk_path(&random_files_dir)?;
        manager._mark_paid_all();
        manager._mark_verified_all();

        let mut new_manager = ChunkManager::new(&root_dir);
        new_manager.chunk_path(&random_files_dir)?;

        // 1. make sure we don't have any unpaid chunks
        assert_eq!(new_manager.unpaid_chunks.len(), 0);
        // 2. we should have paid entries, but 0 chunks inside them
        assert_eq!(new_manager.paid_chunks.len(), 5);
        assert_eq!(
            new_manager
                .paid_chunks
                .values()
                .flat_map(|chunked_file| &chunked_file.chunks)
                .count(),
            0
        );
        // 3. the resumed stats should be 0
        assert_eq!(new_manager.resumed_paid_chunk_count, 0);
        assert_eq!(new_manager.resumed_unpaid_chunk_count, 0);

        // 4. make sure the files are added to verified list
        assert_eq!(new_manager.verified_files.len(), 5);

        Ok(())
    }

    #[test]
    fn metadata_file_absent_from_unpaid_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _root_dir, random_files_dir) = init_manager()?;

        let mut random_files = create_random_files(&random_files_dir, 1, 1)?;
        let random_file = random_files.remove(0);
        manager.chunk_path(&random_files_dir)?;

        let mut old_chunks_from_dir = BTreeSet::new();
        for entry in WalkDir::new(&manager.unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                old_chunks_from_dir.insert(chunk_xorname_from_filename);
            }
        }

        // remove metadata file
        let path_xor = PathXorName::new(&random_file).0;
        let metadata_path = manager.unpaid_dir.join(&path_xor).join(METADATA_FILE);
        fs::remove_file(&metadata_path)?;
        // also remove a random chunk to make sure it is re-chunked
        let removed_chunk = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("We must have 1 file here")
            .chunks
            .iter()
            .next()
            .expect("We must have at least 1 chunk")
            .0;
        let removed_chunk_path = manager
            .unpaid_dir
            .join(path_xor)
            .join(ChunkManager::hex_encode_xorname(removed_chunk));
        fs::remove_file(removed_chunk_path)?;

        // use the same manager to chunk the path
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);
        manager.chunk_path(&random_files_dir)?;
        // nothing should be resumed
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);

        let mut new_chunks_from_dir = BTreeSet::new();
        for entry in WalkDir::new(&manager.unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                new_chunks_from_dir.insert(chunk_xorname_from_filename);
            }
        }

        assert!(new_chunks_from_dir.contains(&removed_chunk));
        assert_list_eq(old_chunks_from_dir, new_chunks_from_dir);
        assert!(metadata_path.exists());

        Ok(())
    }

    #[test]
    fn metadata_file_absent_from_unpaid_dir_but_present_in_paid_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _root_dir, random_files_dir) = init_manager()?;

        let mut random_files = create_random_files(&random_files_dir, 1, 1)?;
        let random_file = random_files.remove(0);
        manager.chunk_path(&random_files_dir)?;

        // mark a file as paid to move it to PAID_DIR and create the metadata file there
        let moved_chunk = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("We must have 1 file here")
            .chunks
            .iter()
            .next()
            .expect("We must have at least 1 chunk")
            .0;
        manager.mark_paid(vec![moved_chunk].into_iter());

        let mut old_unpaid_chunks = BTreeSet::new();
        for entry in WalkDir::new(&manager.unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                old_unpaid_chunks.insert(chunk_xorname_from_filename);
            }
        }
        let mut old_paid_chunks = BTreeSet::new();
        for entry in WalkDir::new(&manager.paid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                old_paid_chunks.insert(chunk_xorname_from_filename);
            }
        }

        // remove metadata file from unpaid dir
        let path_xor = PathXorName::new(&random_file);
        let metadata_path = manager.unpaid_dir.join(path_xor.0).join(METADATA_FILE);
        fs::remove_file(metadata_path)?;

        // use the same manager to chunk the path
        manager.chunk_path(&random_files_dir)?;
        // nothing should be resumed
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);
        assert_eq!(manager.resumed_paid_chunk_count, 1);
        assert_eq!(
            manager
                .unpaid_chunks
                .values()
                .flat_map(|c| &c.chunks)
                .count(),
            0
        );
        assert_eq!(
            manager.paid_chunks.values().flat_map(|c| &c.chunks).count(),
            1
        );

        // the dir should still contain chunks, but never read
        let mut new_unpaid_chunks = BTreeSet::new();
        for entry in WalkDir::new(&manager.unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                new_unpaid_chunks.insert(chunk_xorname_from_filename);
            }
        }
        assert_list_eq(new_unpaid_chunks, old_unpaid_chunks);

        // now marking the chunk as verified should complete the entire file
        // this is intended. Lack of metadata file would make us believe that all the chunks were paid.
        manager.mark_verified(vec![moved_chunk].into_iter());
        manager.chunk_path(&random_files_dir)?;
        assert_eq!(manager.resumed_unpaid_chunk_count, 0);
        assert_eq!(manager.resumed_paid_chunk_count, 0);
        assert_eq!(manager.verified_files.len(), 1);

        Ok(())
    }

    #[test]
    fn metadata_file_absent_from_paid_dir() -> Result<()> {
        let _log_guards = LogBuilder::init_single_threaded_tokio_test("chunk_manager");
        let (_tmp_dir, mut manager, _root_dir, random_files_dir) = init_manager()?;

        let mut random_files = create_random_files(&random_files_dir, 1, 1)?;
        let random_file = random_files.remove(0);
        manager.chunk_path(&random_files_dir)?;

        // mark a file as paid to move it to PAID_DIR and create the metadata file there
        let moved_chunk = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("We must have 1 file here")
            .chunks
            .iter()
            .next()
            .expect("We must have at least 1 chunk")
            .0;
        manager.mark_paid(vec![moved_chunk].into_iter());

        let mut old_unpaid_chunks = BTreeSet::new();
        for entry in WalkDir::new(&manager.unpaid_dir).into_iter().flatten() {
            let file_name = entry.file_name();
            if entry.file_type().is_file() && file_name != METADATA_FILE {
                let chunk_xorname_from_filename =
                    ChunkManager::hex_decode_xorname(file_name.to_str().unwrap())
                        .expect("Failed to get xorname from hex encoded file_name");
                old_unpaid_chunks.insert(chunk_xorname_from_filename);
            }
        }

        // remove metadata file from PAID_DIR
        let path_xor = PathXorName::new(&random_file);
        let metadata_path = manager.paid_dir.join(path_xor.0).join(METADATA_FILE);
        fs::remove_file(metadata_path)?;

        // use the same manager to chunk the path
        manager.chunk_path(&random_files_dir)?;
        // unpaid should be resumed
        assert_eq!(manager.resumed_unpaid_chunk_count, old_unpaid_chunks.len());
        // paid should not be resumed
        assert_eq!(manager.resumed_paid_chunk_count, 0);

        // moving another chunk to paid does not insert the metadata file.
        // should this be fixed? if, need a better way to check for metadata file.
        let moved_chunk = manager
            .unpaid_chunks
            .values()
            .next()
            .expect("We must have 1 file here")
            .chunks
            .iter()
            .next()
            .expect("We must have at least 1 chunk")
            .0;
        manager.mark_paid(vec![moved_chunk].into_iter());

        // use the same manager to chunk the path
        manager.chunk_path(&random_files_dir)?;
        // unpaid should be resumed
        assert_eq!(
            manager.resumed_unpaid_chunk_count,
            old_unpaid_chunks.len() - 1
        );
        // the metadata file is not inserted again.
        assert_eq!(manager.resumed_paid_chunk_count, 0);

        Ok(())
    }

    fn init_manager() -> Result<(TempDir, ChunkManager, PathBuf, PathBuf)> {
        let tmp_dir = tempfile::tempdir()?;
        let random_files_dir = tmp_dir.path().join("random_files");
        let root_dir = tmp_dir.path().join("root_dir");
        fs::create_dir_all(&random_files_dir)?;
        fs::create_dir_all(&root_dir)?;
        let manager = ChunkManager::new(&root_dir);

        Ok((tmp_dir, manager, root_dir, random_files_dir))
    }

    fn create_random_files(
        at: &Path,
        num_files: usize,
        mb_per_file: usize,
    ) -> Result<Vec<PathBuf>> {
        let files = (0..num_files)
            .into_par_iter()
            .filter_map(|i| {
                let mut path = at.to_path_buf();
                path.push(format!("random_file_{i}"));
                match generate_file(&path, mb_per_file) {
                    Ok(_) => Some(path),
                    Err(err) => {
                        error!("Failed to generate random file with {err:?}");
                        None
                    }
                }
            })
            .collect::<Vec<_>>();
        if files.len() < num_files {
            return Err(eyre!("Failed to create a Failedkk"));
        }
        Ok(files)
    }

    fn generate_file(path: &PathBuf, file_size_mb: usize) -> Result<()> {
        let mut file = File::create(path)?;
        let mut rng = thread_rng();

        // can create [u8; 32] max at time. Thus each mb has 1024*32 such small chunks
        let n_small_chunks = file_size_mb * 1024 * 32;
        for _ in 0..n_small_chunks {
            let random_data: [u8; 32] = rng.gen();
            file.write_all(&random_data)?;
        }
        let size = file.metadata()?.len() as f64 / (1024 * 1024) as f64;
        assert_eq!(file_size_mb as f64, size);

        Ok(())
    }
}
