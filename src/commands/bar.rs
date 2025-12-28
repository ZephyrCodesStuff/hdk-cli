use std::path::PathBuf;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Bar {
    /// Create a BAR archive
    Create(IOArgs),
    /// Extract a BAR archive
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
        let mut archive_writer = hdk_archive::bar::writer::BarWriter::new(Vec::new());

        let files = common::collect_input_files(input)?;

        for (abs_path, rel_path) in files {
            let data = common::read_file_bytes(&abs_path)?;
            let name_hash = hdk_secure::hash::AfsHash::from_path(&rel_path);

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

        let mut archive_reader = hdk_archive::bar::reader::BarReader::open(file)
            .map_err(|e| format!("failed to open BAR archive: {e}"))?;

        common::create_output_dir(output)?;

        let extracted = common::extract_archive_entries(&mut archive_reader, output, |m| {
            // BAR doesn't preserve original names; extract by hash.
            m.name_hash.to_string().into()
        })?;

        // Keep the existing UX (log count and destination).
        if extracted > 0 {
            println!("Extracted {extracted} entries");
        }

        println!("Extracted {extracted} files to {}", output.display());
        Ok(())
    }
}
