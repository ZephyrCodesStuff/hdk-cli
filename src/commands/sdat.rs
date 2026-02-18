use clap::Subcommand;
use hdk_secure::hash::AfsHash;
use std::path::Path;

use crate::commands::{Execute, IOArgs, common};

#[derive(Subcommand, Debug)]
pub enum Sdat {
    /// Create an SDAT archive
    #[clap(alias = "c")]
    Create(IOArgs),
    /// Extract an SDAT archive
    #[clap(alias = "x")]
    Extract(IOArgs),
}

const SDAT_KEYS: hdk_sdat::SdatKeys = hdk_sdat::SdatKeys {
    sdat_key: [
        0x0D, 0x65, 0x5E, 0xF8, 0xE6, 0x74, 0xA9, 0x8A, 0xB8, 0x50, 0x5C, 0xFA, 0x7D, 0x01, 0x29,
        0x33,
    ],
    edat_hash_0: [
        0xEF, 0xFE, 0x5B, 0xD1, 0x65, 0x2E, 0xEB, 0xC1, 0x19, 0x18, 0xCF, 0x7C, 0x04, 0xD4, 0xF0,
        0x11,
    ],
    edat_hash_1: [
        0x3D, 0x92, 0x69, 0x9B, 0x70, 0x5B, 0x07, 0x38, 0x54, 0xD8, 0xFC, 0xC6, 0xC7, 0x67, 0x27,
        0x47,
    ],
    edat_key_0: [
        0xBE, 0x95, 0x9C, 0xA8, 0x30, 0x8D, 0xEF, 0xA2, 0xE5, 0xE1, 0x80, 0xC6, 0x37, 0x12, 0xA9,
        0xAE,
    ],
    edat_key_1: [
        0x4C, 0xA9, 0xC1, 0x4B, 0x01, 0xC9, 0x53, 0x09, 0x96, 0x9B, 0xEC, 0x68, 0xAA, 0x0B, 0xC0,
        0x81,
    ],
    npdrm_omac_key_2: [
        0x6B, 0xA5, 0x29, 0x76, 0xEF, 0xDA, 0x16, 0xEF, 0x3C, 0x33, 0x9F, 0xB2, 0x97, 0x1E, 0x25,
        0x6B,
    ],
    npdrm_omac_key_3: [
        0x9B, 0x51, 0x5F, 0xEA, 0xCF, 0x75, 0x06, 0x49, 0x81, 0xAA, 0x60, 0x4D, 0x91, 0xA5, 0x4E,
        0x97,
    ],
};

impl Execute for Sdat {
    fn execute(self) {
        let function = match self {
            Self::Create(args) => Self::create(&args.input, &args.output),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
        };

        if let Err(e) = function {
            eprintln!("Error: {}", e);
        }
    }
}

impl Sdat {
    pub fn create(input: &Path, output: &Path) -> Result<(), String> {
        // TODO: let user pick if SHARC or BAR
        // TODO: let user pick endianness
        let mut archive_writer = hdk_archive::sharc::writer::SharcWriter::new(
            Vec::new(),
            crate::keys::SHARC_SDAT_KEY,
            hdk_archive::structs::Endianness::Big,
        )
        .map_err(|e| format!("failed to create SHARC writer: {e}"))?;

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
                    // TODO: let user pick how to compress/encrypt files
                    hdk_archive::structs::CompressionType::Encrypted,
                    &data,
                )
                .map_err(|e| format!("failed to add entry to SDAT: {e}"))?;
        }

        // Finalize SHARC archive
        let archive_bytes = archive_writer
            .finish()
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        // Wrap SHARC in SDAT
        let output_file_name = output
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid output file name")?
            .to_string();

        let output_file = common::create_output_file(output)?;

        let sdat = hdk_sdat::SdatWriter::new(output_file_name, SDAT_KEYS)
            .map_err(|e| format!("failed to create SDAT writer: {e}"))?;

        let sdat_bytes = sdat
            .write_to_vec(&archive_bytes)
            .map_err(|e| format!("failed to write SDAT: {e}"))?;

        // Write SDAT to output file
        std::io::copy(&mut &sdat_bytes[..], &mut &output_file)
            .map_err(|e| format!("failed to write SDAT to file: {e}"))?;

        println!("Created SDAT archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        // Open and read the SDAT file
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        // Parse the SDAT file to extract the SHARC/BAR archive
        let mut sdat = hdk_sdat::SdatReader::open(file, &SDAT_KEYS)
            .map_err(|e| format!("failed to open SDAT: {e}"))?;

        let archive_bytes = sdat
            .decrypt_to_vec()
            .map_err(|e| format!("failed to decrypt SDAT: {e}"))?;

        // Try SHARC first, then BAR. If neither work, return error.
        // Each attempt gets its own cursor because opening may consume/inspect it.
        // Try SHARC
        if let Ok(mut archive_reader) = hdk_archive::sharc::reader::SharcReader::open(
            std::io::Cursor::new(archive_bytes.clone()),
            crate::keys::SHARC_SDAT_KEY,
        ) {
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
            return Ok(());
        }

        // Try BAR
        if let Ok(mut archive_reader) = hdk_archive::bar::reader::BarReader::open(
            std::io::Cursor::new(archive_bytes),
            crate::keys::BAR_DEFAULT_KEY,
            crate::keys::BAR_SIGNATURE_KEY,
            None,
        ) {
            common::create_output_dir(output)?;

            let extracted = common::extract_archive_entries(&mut archive_reader, output, |m| {
                m.name_hash.to_string().into()
            })?;

            // Save the `.time` as LE (since in the future we won't know what endianness the archive had)
            let time = archive_reader.header().timestamp;
            let time_path = output.join(".time");

            std::fs::write(&time_path, time.to_le_bytes())
                .map_err(|e| format!("failed to write .time file: {e}"))?;

            println!("Extracted {extracted} files to {}", output.display());
            return Ok(());
        }

        Err("file does not contain a supported SHARC or BAR archive".to_string())
    }
}
