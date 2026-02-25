use std::path::{Path, PathBuf};
use std::sync::Arc;

use binrw::{BinRead, Endian};
use clap::Subcommand;
use rand::RngExt;

use hdk_archive::{
    bar::structs::BarArchive,
    sharc::{builder::SharcBuilder, structs::SharcArchive},
    structs::{ArchiveFlags, ArchiveFlagsValue, CompressionType, Endianness},
};

use crate::{
    commands::{ArchiveType, CompressedFile, EndianArg, Execute, IArg, IOArgs, common},
    keys::{SHARC_FILES_KEY, SHARC_SDAT_KEY},
    magic,
};

#[cfg(feature = "rayon")]
use rayon::prelude::*;

#[derive(Subcommand, Debug)]
pub enum Sdat {
    /// Create an SDAT archive
    #[clap(alias = "c")]
    Create {
        /// Input directory to create SDAT from
        #[clap(short, long)]
        input: PathBuf,

        /// Output SDAT file path
        #[clap(short, long)]
        output: PathBuf,

        /// Archive type (SHARC or BAR) to wrap in SDAT (default: SHARC)
        #[clap(short, long, default_value = "sharc")]
        archive_type: ArchiveType,

        /// Endianness for the inner SHARC/BAR archive (default: big-endian)
        #[clap(short, long, default_value = "big")]
        endian: EndianArg,

        /// Whether to protect the inner SHARC/BAR archive
        #[clap(short, long, default_value_t = false)]
        protect: bool,
    },
    /// Extract an SDAT archive
    #[clap(alias = "x")]
    Extract(IOArgs),
    /// Inspect an SDAT archive and print its contents
    #[clap(alias = "i")]
    Inspect(IArg),
}

const SDAT_KEYS: hdk_sdat::SdatKeys = hdk_sdat::SdatKeys {
    sdat_key: [
        0x0D, 0x65, 0x5E, 0xF8, 0xE6, 0x74, 0xA9, 0x8A, 0xB8, 0x50, 0x5C, 0xFA, 0x7D, 0x01, 0x29,
        0x33,
    ],
    edat_hash_0: [
        0xEF, 0xFE, 0x5B, 0xD1, 0x65, 0x2E, 0xEB, 0xC1, 0x19, 0x18, 0xCF, 0x7C, 0x04, 0xD4, 0xF0,
        0x11,
    ],
    edat_hash_1: [
        0x3D, 0x92, 0x69, 0x9B, 0x70, 0x5B, 0x07, 0x38, 0x54, 0xD8, 0xFC, 0xC6, 0xC7, 0x67, 0x27,
        0x47,
    ],
    edat_key_0: [
        0xBE, 0x95, 0x9C, 0xA8, 0x30, 0x8D, 0xEF, 0xA2, 0xE5, 0xE1, 0x80, 0xC6, 0x37, 0x12, 0xA9,
        0xAE,
    ],
    edat_key_1: [
        0x4C, 0xA9, 0xC1, 0x4B, 0x01, 0xC9, 0x53, 0x09, 0x96, 0x9B, 0xEC, 0x68, 0xAA, 0x0B, 0xC0,
        0x81,
    ],
    npdrm_omac_key_2: [
        0x6B, 0xA5, 0x29, 0x76, 0xEF, 0xDA, 0x16, 0xEF, 0x3C, 0x33, 0x9F, 0xB2, 0x97, 0x1E, 0x25,
        0x6B,
    ],
    npdrm_omac_key_3: [
        0x9B, 0x51, 0x5F, 0xEA, 0xCF, 0x75, 0x06, 0x49, 0x81, 0xAA, 0x60, 0x4D, 0x91, 0xA5, 0x4E,
        0x97,
    ],
};

impl Execute for Sdat {
    fn execute(self) {
        let function = match self {
            Self::Create {
                input,
                output,
                archive_type,
                endian,
                protect,
            } => Self::create(&input, &output, archive_type, endian, protect),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
            Self::Inspect(args) => Self::inspect(&args.input),
        };

        if let Err(e) = function {
            eprintln!("Error: {}", e);
        }
    }
}

impl Sdat {
    pub fn create(
        input: &Path,
        output: &Path,
        _archive_type: ArchiveType,
        endian: EndianArg,
        protect: bool,
    ) -> Result<(), String> {
        let endianess = Endianness::from(endian);
        let flags = if protect {
            ArchiveFlags(ArchiveFlagsValue::Protected.into())
        } else {
            ArchiveFlags::default()
        };

        let mut archive_writer =
            SharcBuilder::new(SHARC_SDAT_KEY, SHARC_FILES_KEY).with_flags(flags);

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

        let _ = common::create_output_file(output)?;
        let mut files = common::collect_input_files(input)?;

        // Sort by signed AfsHash value (ascending)
        files.sort_by_key(|a| a.2.0);

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

        // Finalize SHARC archive
        let mut buf = Vec::new();
        let mut writer = std::io::Cursor::new(&mut buf);

        archive_writer
            .build(&mut writer, endianess.into())
            .map_err(|e| format!("failed to finalize SHARC: {e}"))?;

        // Wrap SHARC in SDAT
        let output_file_name = output
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("invalid output file name")?
            .to_string();

        let sdat = hdk_sdat::SdatWriter::new(output_file_name, SDAT_KEYS)
            .map_err(|e| format!("failed to create SDAT writer: {e}"))?;

        let sdat_bytes = sdat
            .write_to_vec(&buf)
            .map_err(|e| format!("failed to write SDAT: {e}"))?;

        // Write SDAT to output file
        std::fs::write(output, &sdat_bytes)
            .map_err(|e| format!("failed to write output file: {e}"))?;

        println!("Created SDAT archive: {}", output.display());
        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        // Open and read the SDAT file
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        // Parse the SDAT file to extract the SHARC/BAR archive
        let mut sdat = hdk_sdat::SdatReader::open(file, &SDAT_KEYS)
            .map_err(|e| format!("failed to open SDAT: {e}"))?;

        let archive_bytes = sdat
            .decrypt_to_vec()
            .map_err(|e| format!("failed to decrypt SDAT: {e}"))?;

        // Try SHARC first, then BAR. If neither work, return error.
        let magic: &[u8; 4] = &archive_bytes[0..4].try_into().unwrap();
        let endian: Endian = magic::magic_to_endianess(magic).into();

        // Share archive bytes across threads if rayon is enabled
        let shared = Arc::new(archive_bytes);
        let mut reader = std::io::Cursor::new(&shared[..]);

        if let Ok(sharc) = match endian {
            Endian::Little => {
                SharcArchive::read_le_args(&mut reader, (SHARC_SDAT_KEY, shared.len() as u32))
            }
            Endian::Big => {
                SharcArchive::read_be_args(&mut reader, (SHARC_SDAT_KEY, shared.len() as u32))
            }
        } {
            common::create_output_dir(output)?;

            #[cfg(not(feature = "rayon"))]
            let results: Vec<(String, Vec<u8>)> = sharc
                .entries
                .iter()
                .map(|entry| {
                    let mut local_reader = std::io::Cursor::new(&shared[..]);
                    let data = sharc
                        .entry_data(&mut local_reader, entry)
                        .expect("Failed to process entry");

                    (entry.name_hash.to_string(), data)
                })
                .collect();

            #[cfg(feature = "rayon")]
            let results: Vec<(String, Vec<u8>)> = sharc
                .entries
                .par_iter()
                .map(|entry| {
                    let mut local_reader = std::io::Cursor::new(&shared[..]);
                    let extracted_data = sharc
                        .entry_data(&mut local_reader, entry)
                        .expect("Failed to process entry");

                    (entry.name_hash.to_string(), extracted_data)
                })
                .collect();

            for (rel, data) in results {
                let output_path = output.join(rel);
                let mut output_file = std::fs::File::create(&output_path).map_err(|e| {
                    format!(
                        "failed to create output file {}: {e}",
                        output_path.display()
                    )
                })?;

                std::io::copy(&mut &data[..], &mut output_file).map_err(|e| {
                    format!("failed to write output file {}: {e}", output_path.display())
                })?;
            }

            let time = sharc.archive_data.timestamp;
            let time_path = output.join(".time");

            std::fs::write(&time_path, time.to_be_bytes())
                .map_err(|e| format!("failed to write .time file: {e}"))?;

            println!(
                "Extracted {} files to {}",
                sharc.entries.len(),
                output.display()
            );
            return Ok(());
        }

        // Try BAR
        // Recreate reader from shared bytes for BAR parsing
        let mut reader = std::io::Cursor::new(&shared[..]);
        if let Ok(bar) = match endian {
            Endian::Little => BarArchive::read_le_args(
                &mut reader,
                (
                    crate::keys::BAR_DEFAULT_KEY,
                    crate::keys::BAR_SIGNATURE_KEY,
                    shared.len() as u32,
                ),
            ),
            Endian::Big => BarArchive::read_be_args(
                &mut reader,
                (
                    crate::keys::BAR_DEFAULT_KEY,
                    crate::keys::BAR_SIGNATURE_KEY,
                    shared.len() as u32,
                ),
            ),
        } {
            common::create_output_dir(output)?;

            #[cfg(not(feature = "rayon"))]
            {
                for entry in &bar.entries {
                    let mut local_reader = std::io::Cursor::new(&shared[..]);
                    let data = bar
                        .entry_data(
                            &mut local_reader,
                            entry,
                            &crate::keys::BAR_DEFAULT_KEY,
                            &crate::keys::BAR_SIGNATURE_KEY,
                        )
                        .map_err(|e| format!("failed to read BAR entry data: {e}"))?;

                    let rel_path = entry.name_hash.to_string();
                    let output_path = output.join(rel_path);

                    let mut output_file = std::fs::File::create(&output_path).map_err(|e| {
                        format!(
                            "failed to create output file {}: {e}",
                            output_path.display()
                        )
                    })?;

                    std::io::copy(&mut &data[..], &mut output_file).map_err(|e| {
                        format!("failed to write output file {}: {e}", output_path.display())
                    })?;
                }
            }

            #[cfg(feature = "rayon")]
            {
                let results: Vec<(String, Vec<u8>)> = bar
                    .entries
                    .par_iter()
                    .map(|entry| {
                        let local = shared.clone();
                        let mut local_reader = std::io::Cursor::new(&local[..]);
                        let extracted_data = bar
                            .entry_data(
                                &mut local_reader,
                                entry,
                                &crate::keys::BAR_DEFAULT_KEY,
                                &crate::keys::BAR_SIGNATURE_KEY,
                            )
                            .expect("Failed to process entry");
                        (entry.name_hash.to_string(), extracted_data)
                    })
                    .collect();

                for (rel, data) in results {
                    let output_path = output.join(rel);
                    let mut output_file = std::fs::File::create(&output_path).map_err(|e| {
                        format!(
                            "failed to create output file {}: {e}",
                            output_path.display()
                        )
                    })?;

                    std::io::copy(&mut &data[..], &mut output_file).map_err(|e| {
                        format!("failed to write output file {}: {e}", output_path.display())
                    })?;
                }
            }

            let time = bar.archive_data.timestamp;
            let time_path = output.join(".time");

            std::fs::write(&time_path, time.to_be_bytes())
                .map_err(|e| format!("failed to write .time file: {e}"))?;

            println!(
                "Extracted {} files to {}",
                bar.entries.len(),
                output.display()
            );

            return Ok(());
        }

        Err("file does not contain a supported SHARC or BAR archive".to_string())
    }

    pub fn inspect(input: &Path) -> Result<(), String> {
        // Open and read the SDAT file
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;

        // Parse the SDAT file to extract the SHARC/BAR archive
        let mut sdat = hdk_sdat::SdatReader::open(file, &SDAT_KEYS)
            .map_err(|e| format!("failed to open SDAT: {e}"))?;

        let archive_bytes = sdat
            .decrypt_to_vec()
            .map_err(|e| format!("failed to decrypt SDAT: {e}"))?;

        // Try SHARC first
        let magic: &[u8; 4] = &archive_bytes[0..4].try_into().unwrap();
        let endian: Endian = magic::magic_to_endianess(magic).into();
        let mut reader = std::io::Cursor::new(archive_bytes.clone());

        if let Ok(sharc) = match endian {
            Endian::Little => SharcArchive::read_le_args(
                &mut reader,
                (SHARC_SDAT_KEY, archive_bytes.len() as u32),
            ),
            Endian::Big => SharcArchive::read_be_args(
                &mut reader,
                (SHARC_SDAT_KEY, archive_bytes.len() as u32),
            ),
        } {
            let header = sharc.archive_data;
            println!("Archive Type: SHARC");
            println!("Timestamp: {}", header.timestamp);
            println!("Entry Count: {}", sharc.entries.len());
            println!("\nEntries:");
            for entry in &sharc.entries {
                println!(
                    "  - Hash: {}, Offset: {}, Uncompressed Size: {}, Compressed Size: {}",
                    entry.name_hash,
                    entry.location.0,
                    entry.uncompressed_size,
                    entry.compressed_size
                );
            }
            return Ok(());
        }

        // Try BAR
        if let Ok(bar) = match endian {
            Endian::Little => BarArchive::read_le_args(
                &mut reader,
                (
                    crate::keys::BAR_DEFAULT_KEY,
                    crate::keys::BAR_SIGNATURE_KEY,
                    archive_bytes.len() as u32,
                ),
            ),
            Endian::Big => BarArchive::read_be_args(
                &mut reader,
                (
                    crate::keys::BAR_DEFAULT_KEY,
                    crate::keys::BAR_SIGNATURE_KEY,
                    archive_bytes.len() as u32,
                ),
            ),
        } {
            let header = bar.archive_data;
            println!("Archive Type: BAR");
            println!("Timestamp: {}", header.timestamp);
            println!("Entry Count: {}", bar.entries.len());
            println!("\nEntries:");
            for entry in &bar.entries {
                println!(
                    "  - Hash: {}, Offset: {}, Uncompressed Size: {}, Compressed Size: {}",
                    entry.name_hash,
                    entry.location.0,
                    entry.uncompressed_size,
                    entry.compressed_size
                );
            }
            return Ok(());
        }

        Err("file does not contain a supported SHARC or BAR archive".to_string())
    }
}
