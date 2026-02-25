use std::{io::Write, path::Path};

use binrw::{BinRead, Endian};
use clap::Subcommand;
use rand::RngExt;

use hdk_archive::{
    sharc::{builder::SharcBuilder, structs::SharcArchive},
    structs::{CompressionType, Endianness},
};

use crate::{
    commands::{CompressedFile, Execute, IOArgs, common},
    keys::{SHARC_DEFAULT_KEY, SHARC_FILES_KEY},
    magic,
};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

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

        let mut archive_writer = SharcBuilder::new(SHARC_DEFAULT_KEY, SHARC_FILES_KEY);
        let mut output_file = common::create_output_file(output)?;

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

        #[cfg(not(feature = "rayon"))]
        let compressed_data: Vec<CompressedFile> = files
            .into_iter()
            .map(|(abs_path, rel_path, name_hash)| {
                use hdk_archive::structs::CompressionType;

                let iv = {
                    let mut iv = [0u8; 8];
                    let mut rng = rand::rng();
                    rng.fill(&mut iv);
                    iv
                };

                let data = common::read_file_bytes(&abs_path).expect("failed to read input file");
                let compressed = archive_writer
                    .compress_data(&data, CompressionType::Encrypted, &iv)
                    .expect("failed to compress data");

                println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);
                CompressedFile {
                    name_hash,
                    rel_path,
                    uncompressed_size: data.len(),
                    compressed_data: compressed,
                    iv,
                }
            })
            .collect::<Vec<_>>();

        #[cfg(feature = "rayon")]
        let compressed_data: Vec<CompressedFile> = files
            .into_par_iter()
            .map(|(abs_path, rel_path, name_hash)| {
                use hdk_archive::structs::CompressionType;

                let iv = {
                    let mut iv = [0u8; 8];
                    let mut rng = rand::rng();
                    rng.fill(&mut iv);
                    iv
                };

                let data = common::read_file_bytes(&abs_path).expect("failed to read input file");
                let compressed = archive_writer
                    .compress_data(&data, CompressionType::Encrypted, &iv)
                    .expect("failed to compress data");

                CompressedFile {
                    name_hash,
                    rel_path,
                    uncompressed_size: data.len(),
                    compressed_data: compressed,
                    iv,
                }
            })
            .collect();

        for CompressedFile {
            name_hash,
            rel_path,
            uncompressed_size,
            compressed_data: compressed,
            iv,
        } in compressed_data
        {
            println!("Adding file: {} (hash: {})", rel_path.display(), name_hash);
            archive_writer.add_compressed_entry(
                name_hash,
                compressed,
                uncompressed_size as u32,
                // TODO: let user pick how to compress/encrypt files
                CompressionType::Encrypted,
                iv,
            );
        }

        archive_writer
            .build(&mut output_file, endianess.into())
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        output_file
            .flush()
            .map_err(|e| format!("failed to flush output file: {e}"))?;

        println!("Created SHARC archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        #[cfg(not(feature = "memmap2"))]
        let data = std::fs::read(input).map_err(|e| format!("failed to read input file: {e}"))?;

        #[cfg(feature = "memmap2")]
        let data = {
            let file = std::fs::File::open(input)
                .map_err(|e| format!("failed to open input file: {e}"))?;
            unsafe {
                memmap2::Mmap::map(&file)
                    .map_err(|e| format!("failed to memory-map input file: {e}"))?
            }
        };

        let data_len = data.len() as u32;

        let mut magic = [0u8; 4];
        magic.clone_from_slice(&data[0..4]);

        let mut reader = std::io::Cursor::new(&data);

        // let mut archive_reader =
        //     hdk_archive::sharc::reader::SharcReader::open(file, crate::keys::SHARC_DEFAULT_KEY)
        //         .map_err(|e| format!("failed to open SHARC archive: {e}"))?;

        let endian: Endian = magic::magic_to_endianess(&magic).into();
        let sharc = match endian {
            Endian::Little => {
                SharcArchive::read_le_args(&mut reader, (SHARC_DEFAULT_KEY, data_len))
            }
            Endian::Big => SharcArchive::read_be_args(&mut reader, (SHARC_DEFAULT_KEY, data_len)),
        }
        .map_err(|e| format!("failed to read SHARC archive: {e}"))?;

        common::create_output_dir(output)?;

        #[cfg(not(feature = "rayon"))]
        let results = sharc
            .entries
            .iter()
            .map(|entry| {
                let mut local_reader = std::io::Cursor::new(&data);
                let extracted_data = sharc
                    .entry_data(&mut local_reader, entry)
                    .expect("Failed to process entry");

                (entry.name_hash.to_string(), extracted_data)
            })
            .collect::<Vec<_>>();

        #[cfg(feature = "rayon")]
        let results: Vec<(String, Vec<u8>)> = sharc
            .entries
            .par_iter()
            .map(|entry| {
                // Each thread gets its own view of the data
                let mut local_reader = std::io::Cursor::new(&data);

                let extracted_data = sharc
                    .entry_data(&mut local_reader, entry)
                    .expect("Failed to process entry");

                (entry.name_hash.to_string(), extracted_data)
            })
            .collect();

        for (name_hash, extracted_data) in results {
            let output_file = output.join(name_hash);
            std::fs::write(&output_file, extracted_data)
                .map_err(|e| format!("failed to write output file {}: {e}", output_file.display()))
                .unwrap();
        }

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
