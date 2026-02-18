//! Common utilities for archive commands.

use std::io::Read;
use std::path::{Path, PathBuf};

use hdk_archive::archive::ArchiveReader;

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
pub fn collect_input_files(input: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    if input.is_file() {
        let file_name = input
            .file_name()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("file"));
        return Ok(vec![(input.to_path_buf(), file_name)]);
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

        files.push((abs_path, rel_path));
    }

    files.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(files)
}

/// Reads a file into a byte vector.
pub fn read_file_bytes(path: &Path) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| format!("failed to open file {}: {e}", path.display()))?
        .read_to_end(&mut data)
        .map_err(|e| format!("failed to read file {}: {e}", path.display()))?;
    Ok(data)
}

/// Extract all entries from an `ArchiveReader` into `output_dir`.
///
/// Callers provide a mapping from entry metadata to a relative output path.
///
/// # Arguments
///
/// - `archive`: ArchiveReader implementation to extract from
/// - `output_dir`: Output directory to extract files into
/// - `output_rel_path`: Function mapping entry metadata to relative output path
///
/// # Returns
///
/// The number of extracted entries, or an error string.
pub fn extract_archive_entries<A, F>(
    archive: &mut A,
    output_dir: &Path,
    mut output_rel_path: F,
) -> Result<usize, String>
where
    A: ArchiveReader,
    F: FnMut(&A::Metadata) -> PathBuf,
{
    archive
        .for_each_entry(|mut entry| -> std::io::Result<()> {
            let rel_path = output_rel_path(&entry.metadata);
            let output_path = output_dir.join(rel_path);

            let mut output_file = std::fs::File::create(&output_path).map_err(|e| {
                std::io::Error::other(format!(
                    "failed to create output file {}: {e}",
                    output_path.display()
                ))
            })?;

            std::io::copy(&mut entry.reader, &mut output_file).map_err(|e| {
                std::io::Error::other(format!(
                    "failed to write entry to {}: {e}",
                    output_path.display()
                ))
            })?;

            Ok(())
        })
        .map_err(|e| format!("failed to extract entries: {e}"))?;

    Ok(archive.entry_count())
}
