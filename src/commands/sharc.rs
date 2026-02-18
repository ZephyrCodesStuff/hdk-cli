use std::path::Path;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;
use hdk_archive::{sharc::writer::SharcWriter, structs::Endianness};
use hdk_secure::hash::AfsHash;

#[derive(Subcommand, Debug)]
pub enum Sharc {
    /// Create a SHARC archive
    #[clap(alias = "c")]
    Create(IOArgs),
    /// Extract a SHARC archive
    #[clap(alias = "x")]
    Extract(IOArgs),
}

impl Execute for Sharc {
    fn execute(self) {
        let result = match self {
            Self::Create(args) => Self::create(&args.input, &args.output),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Sharc {
    pub fn create(input: &Path, output: &Path) -> Result<(), String> {
        // TODO: let user pick endianness
        let endianess = Endianness::Big;

        let mut archive_writer = SharcWriter::default()
            .with_key(crate::keys::SHARC_DEFAULT_KEY)
            .with_endianess(endianess);

        let files = common::collect_input_files(input)?;

        // Check if the input directory has a `.time` file for timestamp.
        // If so, parse as i32 and use it as the archive timestamp.
        let time_path = input.join(".time");
        if time_path.exists() {
            let time_bytes = common::read_file_bytes(&time_path)?;
            if time_bytes.len() == 4 {
                let timestamp = i32::from_le_bytes(time_bytes.try_into().unwrap());
                archive_writer = archive_writer.with_timestamp(timestamp);
                println!("Using timestamp from .time file: {}", timestamp);
            } else {
                println!(
                    "Warning: .time file has invalid length, using default timestamp (system time)."
                );
            }
        }

        for (abs_path, rel_path) in files {
            let data = common::read_file_bytes(&abs_path)?;

            // Determine the name hash:
            //
            // - If the relative path is an 8-character hex string, treat it as an unmapped hash and parse it directly.
            // - Otherwise, normalize the path (lowercase + forward slashes) and hash it as a mapped entry.
            let raw_path_str = rel_path.to_string_lossy();
            let name_hash =
                if raw_path_str.len() == 8 && raw_path_str.chars().all(|c| c.is_ascii_hexdigit()) {
                    // UNMAPPED: Parse the 8-character hex string directly back into an i32
                    let hash_val = u32::from_str_radix(&raw_path_str, 16).unwrap();
                    AfsHash(hash_val as i32)
                } else {
                    // MAPPED: Normalize the real path (lowercase + forward slashes) and hash it
                    let clean_path = raw_path_str.to_lowercase().replace("\\", "/");
                    AfsHash::new_from_str(&clean_path)
                };

            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

            archive_writer
                .add_entry_from_bytes(
                    name_hash,
                    hdk_archive::structs::CompressionType::Encrypted,
                    &data,
                )
                .map_err(|e| format!("failed to add entry: {e}"))?;
        }

        let mut archive_bytes = archive_writer
            .finish()
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        let output_file = common::create_output_file(output)?;
        std::io::copy(&mut archive_bytes, &mut &output_file)
            .map_err(|e| format!("failed to write archive: {e}"))?;

        println!("Created SHARC archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        let mut archive_reader =
            hdk_archive::sharc::reader::SharcReader::open(file, crate::keys::SHARC_DEFAULT_KEY)
                .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        let extracted = common::extract_archive_entries(&mut archive_reader, output, |m| {
            m.name_hash.to_string().into()
        })?;

        // Save the `.time` with the archive's endianess in the output folder root
        let time = archive_reader.header().timestamp;
        let time_path = output.join(".time");

        std::fs::write(&time_path, time.to_le_bytes())
            .map_err(|e| format!("failed to write .time file: {e}"))?;

        println!("Extracted {extracted} files to {}", output.display());
        Ok(())
    }
}
