//! Common utilities for archive commands.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use hdk_secure::hash::AfsHash;
use smallvec::SmallVec;

/// Confirm overwriting an existing file.
/// Returns `Ok(File)` if the user confirms or file doesn't exist.
/// Returns `Err` if the user declines or an I/O error occurs.
pub fn create_output_file(path: &Path) -> Result<std::fs::File, String> {
    match std::fs::File::create_new(path) {
        Ok(f) => Ok(f),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if dialoguer::Confirm::new()
                .with_prompt(format!(
                    "File `{}` already exists. Overwrite?",
                    path.display()
                ))
                .interact()
                .map_err(|e| format!("failed to read user input: {e}"))?
            {
                std::fs::File::create(path)
                    .map_err(|e| format!("failed to create file {}: {e}", path.display()))
            } else {
                Err(format!(
                    "File `{}` already exists and was not overwritten.",
                    path.display()
                ))
            }
        }
        Err(e) => Err(format!("failed to create file {}: {e}", path.display())),
    }
}

/// Create an output directory, prompting to proceed if it already exists.
pub fn create_output_dir(path: &Path) -> Result<(), String> {
    if path.exists() {
        if !dialoguer::Confirm::new()
            .with_prompt(format!(
                "Output folder `{}` already exists. Proceed?",
                path.display()
            ))
            .interact()
            .map_err(|e| format!("failed to read user input: {e}"))?
        {
            return Err(format!(
                "Output folder `{}` already exists and was not overwritten.",
                path.display()
            ));
        }
    } else {
        std::fs::create_dir_all(path)
            .map_err(|e| format!("failed to create output folder: {e}"))?;
    }
    Ok(())
}

/// Collects all files in a directory (recursively) or returns a single file.
///
/// Calculates and returns the `AfsHash` for each file so callers get a well-formed
/// (absolute path, relative path, name-hash) tuple.
pub fn collect_input_files(input: &Path) -> Result<Vec<(PathBuf, PathBuf, AfsHash)>, String> {
    if input.is_file() {
        let file_name = input
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("file"));

        let raw_path_str = file_name.to_string_lossy().to_string();
        let name_hash =
            if raw_path_str.len() == 8 && raw_path_str.chars().all(|c| c.is_ascii_hexdigit()) {
                let hash_val = hex::decode(&raw_path_str)
                    .map_err(|e| format!("invalid hex in filename '{}': {e}", raw_path_str))?;
                let bytes: [u8; 4] = hash_val
                    .as_slice()
                    .try_into()
                    .map_err(|_| format!("invalid hash bytes length for '{}'", raw_path_str))?;
                AfsHash(i32::from_be_bytes(bytes))
            } else {
                let clean_path = raw_path_str.to_lowercase().replace("\\", "/");
                AfsHash::new_from_str(&clean_path)
            };

        return Ok(vec![(input.to_path_buf(), file_name, name_hash)]);
    }

    if !input.is_dir() {
        return Err(format!("Input path does not exist: {}", input.display()));
    }

    let mut files = Vec::new();
    let walker = walkdir::WalkDir::new(input).into_iter();

    for entry in walker {
        let entry = entry.map_err(|e| format!("failed to read input folder: {e}"))?;
        if !entry.file_type().is_file() {
            continue;
        }

        // If the filename is `.time`, ignore it.
        if entry.file_name() == ".time" {
            println!("Skipping .time file: {}", entry.path().display());
            continue;
        }

        let abs_path = entry.path().to_path_buf();
        let rel_path = entry
            .path()
            .strip_prefix(input)
            .map_err(|e| format!("failed to get relative path: {e}"))?
            .to_path_buf();

        let raw_path_str = rel_path.to_string_lossy().to_string();
        let name_hash =
            if raw_path_str.len() == 8 && raw_path_str.chars().all(|c| c.is_ascii_hexdigit()) {
                let hash_val = hex::decode(&raw_path_str)
                    .map_err(|e| format!("invalid hex in filename '{}': {e}", raw_path_str))?;
                let bytes: [u8; 4] = hash_val
                    .as_slice()
                    .try_into()
                    .map_err(|_| format!("invalid hash bytes length for '{}'", raw_path_str))?;
                hdk_secure::hash::AfsHash(i32::from_be_bytes(bytes))
            } else {
                let clean_path = raw_path_str.to_lowercase().replace("\\", "/");
                hdk_secure::hash::AfsHash::new_from_str(&clean_path)
            };

        files.push((abs_path, rel_path, name_hash));
    }

    Ok(files)
}

/// Reads a file into a byte vector.
pub fn read_file_bytes(path: &Path) -> Result<SmallVec<[u8; 16_384]>, std::io::Error> {
    let mut file = File::open(path)?;
    let metadata = file.metadata()?;
    let size = metadata.len() as usize;

    // Create a SmallVec with the correct capacity
    let mut buffer: SmallVec<[u8; 16384]> = SmallVec::with_capacity(size);

    // Safety: SmallVec doesn't initialize its memory for speed.
    // We use the 'Read' trait to fill the internal space.
    unsafe {
        buffer.set_len(size);
    }

    file.read_exact(&mut buffer)?;
    Ok(buffer)
}
