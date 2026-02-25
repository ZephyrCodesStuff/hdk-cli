use std::path::Path;

use crate::{
    commands::{Execute, IOArgs, common},
    keys::{BAR_DEFAULT_KEY, BAR_SIGNATURE_KEY},
    magic,
};
use binrw::{BinRead, Endian};
use clap::Subcommand;
use hdk_archive::{
    bar::{builder::BarBuilder, structs::BarArchive},
    structs::{ArchiveFlags, ArchiveFlagsValue},
};

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
            Self::Create(args) => Self::create(&args.input, &args.output),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Bar {
    pub fn create(input: &Path, output: &Path) -> Result<(), String> {
        // let mut archive_writer = hdk_archive::bar::writer::BarWriter::default()
        //     .with_default_key(BAR_DEFAULT_KEY)
        //     .with_signature_key(BAR_SIGNATURE_KEY)
        //     .with_flags(ArchiveFlagsValue::Protected.into());
        let mut archive_writer = BarBuilder::new(BAR_DEFAULT_KEY, BAR_SIGNATURE_KEY)
            .with_flags(ArchiveFlags(ArchiveFlagsValue::Protected.into()));

        // Check if the input directory has a `.time` file for timestamp.
        // If so, parse as i32 and use it as the archive timestamp.
        let time_path = input.join(".time");
        if time_path.exists() {
            let time_bytes = common::read_file_bytes(&time_path)
                .map_err(|e| format!("failed to read .time file: {e}"))?;

            if time_bytes.len() == 4 {
                // Always read as BE
                let timestamp = i32::from_be_bytes([
                    time_bytes[0],
                    time_bytes[1],
                    time_bytes[2],
                    time_bytes[3],
                ]);
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
            let data = common::read_file_bytes(&abs_path)
                .map_err(|e| format!("failed to read file {}: {e}", abs_path.display()))?;

            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

            archive_writer.add_entry(
                name_hash,
                data,
                hdk_archive::structs::CompressionType::Encrypted,
            );
        }

        let mut buf = Vec::new();
        let endian = Endian::Little; // TODO: let user pick endianness
        let mut writer = std::io::Cursor::new(&mut buf);

        archive_writer
            .build(&mut writer, endian)
            .map_err(|e| format!("failed to finalize archive: {e}"))?;

        let output_file = common::create_output_file(output)?;
        std::io::copy(&mut buf.as_slice(), &mut &output_file)
            .map_err(|e| format!("failed to write archive: {e}"))?;

        println!("Created BAR archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        let data = common::read_file_bytes(input)
            .map_err(|e| format!("failed to read archive file {}: {e}", input.display()))?;

        let magic: [u8; 4] = data
            .get(0..4)
            .ok_or_else(|| "File too small to be a valid archive".to_string())?
            .try_into()
            .unwrap();
        let endian: Endian = magic::magic_to_endianess(&magic).into();

        common::create_output_dir(output)?;
        let mut reader = std::io::Cursor::new(&data);

        let archive = match endian {
            Endian::Little => BarArchive::read_le_args(
                &mut reader,
                (BAR_DEFAULT_KEY, BAR_SIGNATURE_KEY, data.len() as u32),
            ),
            Endian::Big => BarArchive::read_be_args(
                &mut reader,
                (BAR_DEFAULT_KEY, BAR_SIGNATURE_KEY, data.len() as u32),
            ),
        }
        .map_err(|e| format!("failed to open BAR archive: {e}"))?;

        for entry in &archive.entries {
            let file_data = archive
                .entry_data(&mut reader, entry, &BAR_DEFAULT_KEY, &BAR_SIGNATURE_KEY)
                .map_err(|e| format!("failed to read entry data: {e}"))?;

            let output_path = output.join(format!("{}.bin", entry.name_hash));

            std::fs::write(&output_path, file_data)
                .map_err(|e| format!("failed to write file {}: {e}", output_path.display()))?;
        }

        // Save the `.time` with the archive's endianess in the output folder root
        let time = archive.archive_data.timestamp;
        let time_path = output.join(".time");

        // Always write the timestamp in big-endian for consistency
        std::fs::write(&time_path, time.to_be_bytes())
            .map_err(|e| format!("failed to write .time file: {e}"))?;

        println!(
            "Extracted {} files to {}",
            archive.entries.len(),
            output.display()
        );
        Ok(())
    }
}
