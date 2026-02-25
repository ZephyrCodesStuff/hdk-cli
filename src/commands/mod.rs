use crate::commands::{
    bar::Bar, compress::Compress, crypt::Crypt, map::Map, sdat::Sdat, sharc::Sharc,
};

use hdk_secure::hash::AfsHash;
use smallvec::SmallVec;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand, ValueEnum};
use enum_dispatch::enum_dispatch;

pub mod bar;
pub mod common;
pub mod compress;
pub mod crypt;
pub mod map;
pub mod pkg;
pub mod sdat;
pub mod sharc;

/// CLI for the `hdk-rs` PlayStation Home development kit.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
pub struct Main {
    /// Command to run
    #[command(subcommand)]
    pub command: crate::commands::Command,
}

/// Trait for executing commands.
///
/// Each command enum implements this trait to provide its execution logic.
#[enum_dispatch]
pub trait Execute {
    fn execute(self);
}

/// All of the available commands.
#[derive(Subcommand, Debug)]
#[command(propagate_version = true)]
#[enum_dispatch(Execute)]
pub enum Command {
    /// SDAT file operations
    #[command(subcommand)]
    Sdat(Sdat),

    /// SHARC file operations
    #[command(subcommand)]
    Sharc(Sharc),

    /// BAR file operations
    #[command(subcommand)]
    Bar(Bar),

    /// Cryptographic operations
    #[command(subcommand)]
    Crypt(Crypt),

    /// Compression operations (EdgeZLib / EdgeLZMA)
    #[command(subcommand)]
    Compress(Compress),

    /// Map files and restore original file structures
    #[command()]
    Map(Map),

    /// PKG file operations
    #[command(subcommand)]
    Pkg(pkg::Pkg),
}

#[derive(Args, Debug)]
pub struct Input {
    /// Input file / folder path
    #[clap(short, long)]
    pub input: PathBuf,
}

/// Common input/output arguments for commands.
#[derive(Args, Debug)]
pub struct IOArgs {
    /// Input file / folder path
    #[clap(short, long)]
    pub input: PathBuf,

    /// Output file / folder path
    #[clap(short, long)]
    pub output: PathBuf,
}

/// Common input arguments for commands that only require an input path.
#[derive(Args, Debug)]
pub struct IArg {
    /// Input file / folder path
    #[clap(short, long)]
    pub input: PathBuf,
}

/// Utility wrapping of Endianness for clap argument parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum EndianArg {
    Little,
    Big,
}

impl From<EndianArg> for hdk_archive::structs::Endianness {
    fn from(value: EndianArg) -> Self {
        match value {
            EndianArg::Little => Self::Little,
            EndianArg::Big => Self::Big,
        }
    }
}

/// Archive type parser
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ArchiveType {
    Sharc,
    Bar,
}

pub struct CompressedFile {
    name_hash: AfsHash,
    rel_path: PathBuf,
    uncompressed_size: usize,
    compressed_data: SmallVec<[u8; 16_384]>, // Many entries are below this
    iv: [u8; 8],
}
