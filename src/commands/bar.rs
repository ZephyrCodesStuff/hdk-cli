use std::path::PathBuf;

use crate::{
    commands::{Execute, IOArgs, common},
    keys::{BAR_DEFAULT_KEY, BAR_SIGNATURE_KEY},
};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Bar {
    /// Create a BAR archive
    #[clap(alias = "c")]
    Create(IOArgs),
    /// Extract a BAR archive
    #[clap(alias = "x")]
    Extract(IOArgs),
}

impl Execute for Bar {
    fn execute(self) {
        let result = match self {
            Self::Create(args) => Bar::create(&args.input, &args.output),
            Self::Extract(args) => Bar::extract(&args.input, &args.output),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Bar {
    pub fn create(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        let mut archive_writer = hdk_archive::bar::writer::BarWriter::new(
            Vec::new(),
            BAR_DEFAULT_KEY,
            BAR_SIGNATURE_KEY,
        )
        .unwrap();

        let files = common::collect_input_files(input)?;

        // Check if the input directory has a `.time` file for timestamp.
        // If so, parse as i32 and use it as the archive timestamp.
        let time_path = input.join(".time");
        if time_path.exists() {
            let time_bytes = common::read_file_bytes(&time_path)?;
            if time_bytes.len() == 4 {
                // TODO: when BAR supports endianness, use that instead of LE
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
            let name_hash = hdk_secure::hash::AfsHash::new_from_path(&rel_path);

            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

            archive_writer
                .add_entry(
                    name_hash,
                    hdk_archive::structs::CompressionType::Encrypted,
                    &data,
                )
                .map_err(|e| format!("failed to add entry: {e}"))?;
        }

        let archive_bytes = archive_writer
            .finish()
            .map_err(|e| format!("failed to finalize BAR: {e}"))?;

        let output_file = common::create_output_file(output)?;
        std::io::copy(&mut &archive_bytes[..], &mut &output_file)
            .map_err(|e| format!("failed to write archive: {e}"))?;

        println!("Created BAR archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        let mut archive_reader = hdk_archive::bar::reader::BarReader::open(
            file,
            BAR_DEFAULT_KEY,
            BAR_SIGNATURE_KEY,
            None,
        )
        .map_err(|e| format!("failed to open BAR archive: {e}"))?;

        common::create_output_dir(output)?;

        let extracted = common::extract_archive_entries(&mut archive_reader, output, |m| {
            // BAR doesn't preserve original names; extract by hash.
            m.name_hash.to_string().into()
        })?;

        if extracted > 0 {
            println!("Extracted {extracted} entries");
        }

        // Save the `.time` with the archive's endianess in the output folder root
        let time = archive_reader.header().timestamp;
        let time_path = output.join(".time");
        let time_bytes = time.to_le_bytes(); // TODO: use endianess when BAR supports it

        std::fs::write(&time_path, time_bytes)
            .map_err(|e| format!("failed to write .time file: {e}"))?;

        println!("Extracted {extracted} files to {}", output.display());
        Ok(())
    }
}
