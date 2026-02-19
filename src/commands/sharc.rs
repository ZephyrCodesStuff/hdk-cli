use std::path::Path;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;
use hdk_archive::{
    sharc::writer::SharcWriter,
    structs::{ArchiveFlags, Endianness},
};

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
            .with_endianess(endianess)
            .with_flags(ArchiveFlags::Protected.into());

        // Check if the input directory has a `.time` file for timestamp.
        // If so, parse as i32 and use it as the archive timestamp.
        let time_path = input.join(".time");
        if time_path.exists() {
            let time_bytes = common::read_file_bytes(&time_path)?;
            if time_bytes.len() == 4 {
                // Always read as LE
                let timestamp = i32::from_be_bytes(time_bytes.try_into().unwrap());
                archive_writer = archive_writer.with_timestamp(timestamp);
                println!("Using timestamp from .time file: {}", timestamp);
            } else {
                println!(
                    "Warning: .time file has invalid length, using default timestamp (system time)."
                );
            }
        }

        let mut files = common::collect_input_files(input)?;

        // Sort ascending by signed AfsHash value
        // This ensures they're written in the same order as the input files
        files.sort_by(|(_, _, a_hash), (_, _, b_hash)| a_hash.0.cmp(&b_hash.0));

        for (abs_path, rel_path, name_hash) in files {
            let data = common::read_file_bytes(&abs_path)?;

            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

            archive_writer
                .add_entry_from_bytes(
                    name_hash,
                    hdk_archive::structs::CompressionType::Encrypted,
                    &data,
                )
                .map_err(|e| format!("failed to add entry: {e}"))?;
        }

        let archive_bytes = archive_writer
            .finish()
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        let output_file = common::create_output_file(output)?;
        std::io::copy(&mut archive_bytes.get_ref().as_slice(), &mut &output_file)
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

        // Always write the timestamp in big-endian for consistency
        std::fs::write(&time_path, time.to_be_bytes())
            .map_err(|e| format!("failed to write .time file: {e}"))?;

        println!("Extracted {extracted} files to {}", output.display());
        Ok(())
    }
}
