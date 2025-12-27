use std::path::PathBuf;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Sdat {
    /// Create an SDAT archive
    Create(IOArgs),
    /// Extract an SDAT archive
    Extract(IOArgs),
}

impl Execute for Sdat {
    fn execute(self) {
        let function = match self {
            Self::Create(args) => Sdat::create(&args.input, &args.output),
            Self::Extract(args) => Sdat::extract(&args.input, &args.output),
        };

        if let Err(e) = function {
            eprintln!("Error: {}", e);
        }
    }
}

impl Sdat {
    pub fn create(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        // TODO: let user pick if SHARC or BAR
        // TODO: let user pick endianness
        let mut archive_writer = hdk_archive::sharc::writer::SharcWriter::new(
            Vec::new(),
            crate::keys::SHARC_SDAT_KEY,
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

        let sdat = hdk_sdat::SdatWriter::new(output_file_name)
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

    pub fn extract(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        // Open and read the SDAT file
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        // Parse the SDAT file to extract the SHARC archive
        let mut sdat =
            hdk_sdat::SdatReader::open(file).map_err(|e| format!("failed to open SDAT: {e}"))?;

        let archive_bytes = sdat
            .decrypt_to_vec()
            .map_err(|e| format!("failed to decrypt SDAT: {e}"))?;

        let archive_cursor = std::io::Cursor::new(archive_bytes);

        // TODO: check whether it's a SHARC or BAR archive instead of assuming SHARC
        let mut archive_reader = hdk_archive::sharc::reader::SharcReader::open(
            archive_cursor,
            crate::keys::SHARC_SDAT_KEY,
        )
        .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        // Extract all entries to the output folder
        for i in 0..archive_reader.entries().len() {
            let name_hash = archive_reader.entries()[i].name_hash();
            let output_path = output.join(name_hash.to_string());

            let mut output_file = std::fs::File::create(&output_path)
                .map_err(|e| format!("failed to create output file: {e}"))?;

            let mut entry_reader = archive_reader
                .entry_reader(i)
                .map_err(|e| format!("failed to create entry reader: {e}"))?;

            std::io::copy(&mut entry_reader, &mut output_file)
                .map_err(|e| format!("failed to write entry to file: {e}"))?;

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
