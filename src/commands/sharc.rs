use std::path::PathBuf;

use crate::commands::{Execute, IOArgs, common};
use clap::Subcommand;
use hdk_archive::{sharc::writer::SharcWriter, structs::Endianness};

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
        let mut archive_writer = SharcWriter::default()
            .with_key(crate::keys::SHARC_DEFAULT_KEY)
            .with_endianess(Endianness::Big);

        let files = common::collect_input_files(input)?;

        for (abs_path, rel_path) in files {
            let data = common::read_file_bytes(&abs_path)?;
            let name_hash = hdk_secure::hash::AfsHash::new_from_path(&rel_path);

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

    pub fn extract(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        let mut archive_reader =
            hdk_archive::sharc::reader::SharcReader::open(file, crate::keys::SHARC_DEFAULT_KEY)
                .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        let extracted = common::extract_archive_entries(&mut archive_reader, output, |m| {
            m.name_hash.to_string().into()
        })?;

        println!("Extracted {extracted} files to {}", output.display());
        Ok(())
    }
}
