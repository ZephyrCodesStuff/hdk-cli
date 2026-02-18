use std::fs::File;
use std::io::{self, BufReader, BufWriter, Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::commands::{Execute, common};
use clap::{Subcommand, ValueEnum};

#[derive(Subcommand, Debug)]
#[clap(alias = "comp")]
pub enum Compress {
    /// Compress a file using EdgeZLib or EdgeLZMA
    #[clap(alias = "c")]
    Compress {
        /// Input file path
        #[clap(short, long)]
        input: PathBuf,

        /// Output file path
        #[clap(short, long)]
        output: PathBuf,

        /// Compression algorithm to use
        #[clap(short, long, value_enum, default_value_t = Algorithm::Lzma)]
        algorithm: Algorithm,
    },
    /// Decompress a file compressed with EdgeZLib or EdgeLZMA
    #[clap(alias = "d")]
    Decompress {
        /// Input file path
        #[clap(short, long)]
        input: PathBuf,

        /// Output file path
        #[clap(short, long)]
        output: PathBuf,

        /// Compression algorithm that was used
        #[clap(short, long, value_enum, default_value_t = Algorithm::Lzma)]
        algorithm: Algorithm,
    },
}

#[derive(ValueEnum, Clone, Copy, Debug, Default)]
pub enum Algorithm {
    /// EdgeZLib segmented compression (64KB chunks)
    Zlib,
    /// EdgeLZMA segmented compression (64KB chunks)
    ///
    /// This is the default algorithm.
    #[default]
    Lzma,
}

impl Execute for Compress {
    fn execute(self) {
        let result = match self {
            Self::Compress {
                input,
                output,
                algorithm,
            } => compress(&input, &output, algorithm),
            Self::Decompress {
                input,
                output,
                algorithm,
            } => decompress(&input, &output, algorithm),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

fn compress(input: &Path, output: &Path, algorithm: Algorithm) -> Result<(), String> {
    let input_file = File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;
    let mut reader = BufReader::new(input_file);

    let output_file = common::create_output_file(output)?;
    let writer = BufWriter::new(output_file);

    let bytes_written = match algorithm {
        Algorithm::Zlib => compress_zlib(&mut reader, writer)?,
        Algorithm::Lzma => compress_lzma(&mut reader, writer)?,
    };

    println!(
        "Compressed {} -> {} ({} bytes, {:?})",
        input.display(),
        output.display(),
        bytes_written,
        algorithm
    );
    Ok(())
}

fn decompress(input: &Path, output: &Path, algorithm: Algorithm) -> Result<(), String> {
    let input_file = File::open(input).map_err(|e| format!("failed to open input file: {e}"))?;
    let reader = BufReader::new(input_file);

    let output_file = common::create_output_file(output)?;
    let mut writer = BufWriter::new(output_file);

    let bytes_written = match algorithm {
        Algorithm::Zlib => decompress_zlib(reader, &mut writer)?,
        Algorithm::Lzma => decompress_lzma(reader, &mut writer)?,
    };

    println!(
        "Decompressed {} -> {} ({} bytes, {:?})",
        input.display(),
        output.display(),
        bytes_written,
        algorithm
    );
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Zlib (EdgeZLib segmented)
// ─────────────────────────────────────────────────────────────────────────────

fn compress_zlib<R: Read, W: Write>(reader: &mut R, writer: W) -> Result<u64, String> {
    use hdk_comp::zlib::writer::SegmentedZlibWriter;

    let mut compressor = SegmentedZlibWriter::new(writer);

    io::copy(reader, &mut compressor).map_err(|e| format!("compression failed: {e}"))?;

    let inner = compressor
        .finish()
        .map_err(|e| format!("failed to finalize compressed stream: {e}"))?;

    // Get bytes written (flush first)
    let mut inner = inner;
    inner
        .flush()
        .map_err(|e| format!("failed to flush output: {e}"))?;

    // We don't have direct access to bytes written, so we report success
    Ok(0) // Caller will stat the file if needed
}

fn decompress_zlib<R: Read, W: Write>(reader: R, writer: &mut W) -> Result<u64, String> {
    use hdk_comp::zlib::reader::SegmentedZlibReader;

    let mut decompressor = SegmentedZlibReader::new(reader);

    let bytes =
        io::copy(&mut decompressor, writer).map_err(|e| format!("decompression failed: {e}"))?;

    writer
        .flush()
        .map_err(|e| format!("failed to flush output: {e}"))?;

    Ok(bytes)
}

// ─────────────────────────────────────────────────────────────────────────────
// LZMA (EdgeLZMA segmented)
// ─────────────────────────────────────────────────────────────────────────────

fn compress_lzma<R: Read, W: Write>(reader: &mut R, writer: W) -> Result<u64, String> {
    use hdk_comp::lzma::writer::SegmentedLzmaWriter;

    let mut compressor = SegmentedLzmaWriter::new(writer);

    io::copy(reader, &mut compressor).map_err(|e| format!("compression failed: {e}"))?;

    let inner = compressor
        .finish()
        .map_err(|e| format!("failed to finalize compressed stream: {e}"))?;

    let mut inner = inner;
    inner
        .flush()
        .map_err(|e| format!("failed to flush output: {e}"))?;

    Ok(0)
}

fn decompress_lzma<R: Read + Seek, W: Write>(reader: R, writer: &mut W) -> Result<u64, String> {
    use hdk_comp::lzma::reader::SegmentedLzmaReader;

    let mut decompressor =
        SegmentedLzmaReader::new(reader).map_err(|e| format!("failed to open LZMA stream: {e}"))?;

    let bytes =
        io::copy(&mut decompressor, writer).map_err(|e| format!("decompression failed: {e}"))?;

    writer
        .flush()
        .map_err(|e| format!("failed to flush output: {e}"))?;

    Ok(bytes)
}
