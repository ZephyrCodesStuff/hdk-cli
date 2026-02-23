use std::path::Path;

use crate::{
    commands::{Execute, IOArgs, common},
    keys::{SHARC_DEFAULT_KEY, SHARC_FILES_KEY, SHARC_SDAT_KEY},
    magic,
};
use binrw::{BinRead, Endian};
use clap::Subcommand;
use hdk_archive::{
    sharc::{builder::SharcBuilder, structs::SharcArchive},
    structs::Endianness,
};
use rand::RngExt;

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

        let mut archive_writer = SharcBuilder::new(SHARC_SDAT_KEY, SHARC_FILES_KEY);

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
        files.sort_by_key(|(_, _, a_hash)| a_hash.0);

        for (abs_path, rel_path, name_hash) in files {
            let data = common::read_file_bytes(&abs_path)?;

            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

            let mut iv = [0u8; 8];
            let mut rng = rand::rng();
            rng.fill(&mut iv);

            archive_writer.add_entry(
                name_hash,
                data,
                // TODO: allow user to specify compression type
                hdk_archive::structs::CompressionType::Encrypted,
                iv,
            );
        }

        let mut buf = Vec::new();
        let mut writer = std::io::Cursor::new(&mut buf);

        archive_writer
            .build(&mut writer, endianess.into())
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        let output_file = common::create_output_file(output)?;
        std::io::copy(&mut buf.as_slice(), &mut &output_file)
            .map_err(|e| format!("failed to write archive: {e}"))?;

        println!("Created SHARC archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        let data = std::fs::read(input).map_err(|e| format!("failed to read input file: {e}"))?;
        let mut reader = std::io::Cursor::new(&data);

        // let mut archive_reader =
        //     hdk_archive::sharc::reader::SharcReader::open(file, crate::keys::SHARC_DEFAULT_KEY)
        //         .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        let magic: &[u8; 4] = data[..4]
            .try_into()
            .map_err(|e| format!("failed to read magic: {e}"))?;

        let endian: Endian = magic::magic_to_endianess(magic).into();
        let sharc = match endian {
            Endian::Little => {
                SharcArchive::read_le_args(&mut reader, (SHARC_DEFAULT_KEY, data.len() as u32))
            }
            Endian::Big => {
                SharcArchive::read_be_args(&mut reader, (SHARC_DEFAULT_KEY, data.len() as u32))
            }
        }
        .map_err(|e| format!("failed to read SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        for entry in &sharc.entries {
            let data = sharc
                .entry_data(&mut reader, entry)
                .map_err(|e| format!("failed to read entry data: {e}"))?;

            let output_path = output.join(entry.name_hash.to_string());
            std::fs::write(&output_path, data)
                .map_err(|e| format!("failed to write output file: {e}"))?;
        }

        // Save the `.time` with the archive's endianess in the output folder root
        let time = sharc.archive_data.timestamp;
        let time_path = output.join(".time");

        // Always write the timestamp in big-endian for consistency
        std::fs::write(&time_path, time.to_be_bytes())
            .map_err(|e| format!("failed to write .time file: {e}"))?;

        println!(
            "Extracted {} files to {}",
            sharc.entries.len(),
            output.display()
        );
        Ok(())
    }
}
