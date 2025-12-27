use std::path::PathBuf;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Sharc {
    /// Create a SHARC archive
    Create(IOArgs),
    /// Extract a SHARC archive
    Extract(IOArgs),
}

impl Execute for Sharc {
    fn execute(self) {
        let result = match self {
            Self::Create(args) => Sharc::create(&args.input, &args.output),
            Self::Extract(args) => Sharc::extract(&args.input, &args.output),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Sharc {
    pub fn create(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        // TODO: let user pick endianness
        let mut archive_writer = hdk_archive::sharc::writer::SharcWriter::new(
            Vec::new(),
            crate::keys::SHARC_DEFAULT_KEY,
            hdk_archive::structs::Endianness::Big,
        )
        .map_err(|e| format!("failed to create SHARC writer: {e}"))?;

        let files = common::collect_input_files(input)?;

        for (abs_path, rel_path) in files {
            let data = common::read_file_bytes(&abs_path)?;
            let name_hash = hdk_secure::hash::AfsHash::from_path(&rel_path);

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
        std::io::copy(&mut &archive_bytes[..], &mut &output_file)
            .map_err(|e| format!("failed to write archive: {e}"))?;

        println!("Created SHARC archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        let mut archive_reader =
            hdk_archive::sharc::reader::SharcReader::open(file, crate::keys::SHARC_DEFAULT_KEY)
                .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        for i in 0..archive_reader.entries().len() {
            let name_hash = archive_reader.entries()[i].name_hash();
            let output_path = output.join(name_hash.to_string());

            let mut output_file = std::fs::File::create(&output_path)
                .map_err(|e| format!("failed to create output file: {e}"))?;

            let mut entry_reader = archive_reader
                .entry_reader(i)
                .map_err(|e| format!("failed to create entry reader: {e}"))?;

            std::io::copy(&mut entry_reader, &mut output_file)
                .map_err(|e| format!("failed to write entry: {e}"))?;

            println!("Extracted: {}", name_hash);
        }

        println!(
            "Extracted {} files to {}",
            archive_reader.entries().len(),
            output.display()
        );
        Ok(())
    }
}
