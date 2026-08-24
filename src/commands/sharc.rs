use std::{
    io::Write,
    path::{Path, PathBuf},
};

use binrw::{BinRead, Endian};
use clap::Subcommand;
use rand::RngExt;

use hdk_archive::{
    sharc::{builder::SharcBuilder, structs::{SharcArchive, SharcEntry}},
    structs::{ArchiveFlags, ArchiveFlagsValue, CompressionType, Endianness},
};

use crate::{
    commands::{ArchiveEntryResult, CompressedFile, EndianArg, Execute, IArg, IOArgs, common},
    keys::{SHARC_DEFAULT_KEY, SHARC_FILES_KEY},
    magic,
};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

#[derive(Subcommand, Debug)]
pub enum Sharc {
    /// Create a SHARC archive
    #[clap(alias = "c")]
    Create {
        /// Input directory to create SDAT from
        #[clap(short, long)]
        input: PathBuf,

        /// Output SDAT file path
        #[clap(short, long)]
        output: PathBuf,

        /// Endianness for the SHARC archive (default: big-endian)
        #[clap(short, long, default_value = "big")]
        endian: EndianArg,

        /// Whether to protect the SHARC archive
        #[clap(short, long, default_value_t = false)]
        protect: bool,
    },
    /// Extract a SHARC archive
    #[clap(alias = "x")]
    Extract(IOArgs),
    /// Inspect a SHARC archive and print its contents
    #[clap(alias = "i")]
    Inspect(IArg),
}

impl Execute for Sharc {
    fn execute(self) {
        let result = match self {
            Self::Create {
                input,
                output,
                endian,
                protect,
            } => Self::create(&input, &output, endian, protect),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
            Self::Inspect(args) => Self::inspect(&args.input).map(|info| {
                eprintln!("{info}");
            }),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Sharc {
    pub fn create(
        input: &Path,
        output: &Path,
        endian: EndianArg,
        protect: bool,
    ) -> Result<(), String> {
        let endianness = Endianness::from(endian);
        let flags = if protect {
            ArchiveFlags(ArchiveFlagsValue::Protected.into())
        } else {
            ArchiveFlags::default()
        };

        let mut archive_writer =
            SharcBuilder::new(SHARC_DEFAULT_KEY, SHARC_FILES_KEY).with_flags(flags);

        let mut output_file = common::create_output_file(output)?;

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
                eprintln!("Using timestamp from .time file: {}", timestamp);
            } else {
                eprintln!(
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
            eprintln!("Adding file: {} (hash: {})", rel_path.display(), name_hash);

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
            .build(&mut output_file, endianness.into())
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        output_file
            .flush()
            .map_err(|e| format!("failed to flush output file: {e}"))?;

        eprintln!("Created SHARC archive: {}", output.display());
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
        let results: Vec<ArchiveEntryResult<SharcEntry>> = sharc
            .entries
            .iter()
            .map(|entry| {
                let mut local_reader = std::io::Cursor::new(&data);
                sharc
                    .entry_data(&mut local_reader, entry)
                    .map(|extracted_data| (entry.name_hash.to_string(), extracted_data))
                    .map_err(|e| (entry.name_hash.to_string(), e.to_string(), *entry))
            })
            .collect();

        #[cfg(feature = "rayon")]
        let results: Vec<ArchiveEntryResult<SharcEntry>> = sharc
            .entries
            .par_iter()
            .map(|entry| {
                // Each thread gets its own view of the data
                let mut local_reader = std::io::Cursor::new(&data);

                sharc
                    .entry_data(&mut local_reader, entry)
                    .map(|extracted_data| (entry.name_hash.to_string(), extracted_data))
                    .map_err(|e| (entry.name_hash.to_string(), e.to_string(), *entry))
            })
            .collect();

        let mut successes = Vec::new();
        let mut failures = Vec::new();

        for result in results {
            match result {
                Ok(s) => successes.push(s),
                Err(f) => failures.push(f),
            }
        }

        for (name_hash, extracted_data) in successes {
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

        if !failures.is_empty() {
            eprintln!("\nFailed to extract {} entries:", failures.len());
            for (hash, error, entry) in &failures {
                eprintln!("  - {}: {}\n    Metadata: {:#?}", hash, error, entry);
            }
        }

        eprintln!(
            "\nExtracted {} files to {}",
            sharc.entries.len() - failures.len(),
            output.display()
        );
        Ok(())
    }

    pub fn inspect(input: &Path) -> Result<String, String> {
        use std::fmt::Write;

        let data = std::fs::read(input).map_err(|e| format!("failed to read input file: {e}"))?;
        let data_len = data.len() as u32;

        let mut magic = [0u8; 4];
        magic.clone_from_slice(&data[0..4]);

        let mut reader = std::io::Cursor::new(&data);
        let endian: Endian = magic::magic_to_endianess(&magic).into();

        let sharc = match endian {
            Endian::Little => {
                SharcArchive::read_le_args(&mut reader, (SHARC_DEFAULT_KEY, data_len))
            }
            Endian::Big => SharcArchive::read_be_args(&mut reader, (SHARC_DEFAULT_KEY, data_len)),
        }
        .map_err(|e| format!("failed to read SHARC archive: {e}"))?;

        let header = sharc.archive_data;
        let mut out = String::new();
        let _ = writeln!(out, "Archive Type: SHARC");
        let _ = writeln!(out, "Timestamp: {}", header.timestamp);
        let _ = writeln!(out, "Entry Count: {}", sharc.entries.len());
        let _ = writeln!(out, "\nEntries:");
        for entry in &sharc.entries {
            let _ = writeln!(
                out,
                "  - Hash: {}, Offset: {}, Uncompressed Size: {}, Compressed Size: {}, Compression Type: {:#?}",
                entry.name_hash,
                entry.location.0,
                entry.uncompressed_size,
                entry.compressed_size,
                entry.location.1
            );
        }

        Ok(out)
    }
}
