use clap::{Args, Subcommand};
use hdk_firmware::pkg::{
    PkgContentType, PkgDrmType, PkgPlatform, PkgReleaseType, structs::PkgEntryType,
};
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::commands::{Execute, IOArgs, Input, common};

#[derive(Subcommand, Debug)]
pub enum Pkg {
    /// Inspect a PlayStation 3 PKG file
    #[clap(alias = "i")]
    Inspect(Input),

    /// Extract contents of a PlayStation 3 PKG file
    #[clap(alias = "x")]
    Extract(IOArgs),

    /// Create a PlayStation 3 PKG file from a directory
    #[clap(alias = "c")]
    Create(PkgCreateArgs),
}

impl Execute for Pkg {
    fn execute(self) {
        let function = match self {
            Self::Inspect(args) => Self::inspect(&args.input),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
            Self::Create(args) => Self::create(&args),
        };

        if let Err(e) = function {
            eprintln!("Error: {}", e);
        }
    }
}

impl Pkg {
    pub fn inspect(input: &PathBuf) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open PKG file: {e}"))?;

        let mut pkg = hdk_firmware::pkg::reader::PkgArchive::open(file)
            .map_err(|e| format!("failed to read PKG file: {e}"))?;

        println!("PKG header: {:#?}", pkg.header());

        // Print every metadata packet
        println!("Metadata packets:");
        for packet in &pkg.metadata().packets {
            println!(
                "  ID: {:X}, size: {}, data (hex): {}",
                packet.id,
                packet.data.len(),
                &packet
                    .data
                    .iter()
                    .take(16)
                    .map(|b| format!("0x{:02X}", b))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        for item in pkg.items().filter_map(|item| item.ok()) {
            println!(
                "{} ({:X}), size: {} bytes",
                item.name, item.entry.flags, item.entry.data_size
            );
        }

        Ok(())
    }

    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        let file =
            std::fs::File::open(input).map_err(|e| format!("failed to open PKG file: {e}"))?;

        let mut pkg = hdk_firmware::pkg::reader::PkgArchive::open(file)
            .map_err(|e| format!("failed to read PKG file: {e}"))?;

        let items: Vec<_> = pkg.items().filter_map(|item| item.ok()).collect();
        for item in items {
            let output_path = output.join(&item.name);

            if item.entry.is_directory() {
                std::fs::create_dir_all(&output_path).map_err(|e| {
                    format!("failed to create directory {}: {e}", output_path.display())
                })?;
            } else {
                if let Some(parent) = output_path.parent() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        format!(
                            "failed to create parent directory {}: {e}",
                            parent.display()
                        )
                    })?;
                }

                let mut output_file = std::fs::File::create(&output_path)
                    .map_err(|e| format!("failed to create file {}: {e}", output_path.display()))?;

                let mut data = pkg
                    .item_reader(item.index.try_into().unwrap())
                    .map_err(|e| format!("failed to read item data: {e}"))?;

                std::io::copy(&mut data, &mut output_file)
                    .map_err(|e| format!("failed to write file {}: {e}", output_path.display()))?;
            }
        }

        Ok(())
    }

    pub fn create(args: &PkgCreateArgs) -> Result<(), String> {
        let input = &args.input;
        let output = &args.output;
        if !input.is_dir() {
            return Err(format!("input path {} is not a directory", input.display()));
        }

        let mut builder = hdk_firmware::pkg::writer::PkgBuilder::new()
            .platform(parse_platform(&args.platform)?)
            .content_type(parse_content_type(&args.content_type)?)
            .release_type(parse_release_type(&args.release_type)?)
            .drm_type(parse_drm_type(&args.drm_type)?)
            .content_id(&args.content_id)
            .title_id(&args.title_id)
            .install_directory(&args.title_id);

        fn pkg_path_string(path: &Path) -> String {
            let parts: Vec<String> = path
                .components()
                .filter_map(|component| match component {
                    std::path::Component::Normal(name) => Some(name.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect();
            parts.join("/")
        }

        struct PkgEntry {
            path_str: String,
            abs_path: Option<PathBuf>,
            entry_type: PkgEntryType,
        }

        let mut entries = Vec::new();
        for entry in WalkDir::new(input).min_depth(1) {
            let entry = entry.map_err(|e| format!("failed to read directory entry: {e}"))?;
            let rel_path = entry
                .path()
                .strip_prefix(input)
                .map_err(|e| format!("failed to get relative path: {e}"))?;

            let path_str = pkg_path_string(rel_path);
            if entry.file_type().is_dir() {
                entries.push(PkgEntry {
                    path_str,
                    abs_path: None,
                    entry_type: PkgEntryType::Folder,
                });
            } else if entry.file_type().is_file() {
                entries.push(PkgEntry {
                    path_str,
                    abs_path: Some(entry.path().to_path_buf()),
                    entry_type: PkgEntryType::Regular,
                });
            }
        }

        entries.sort_by(|a, b| a.path_str.cmp(&b.path_str));

        for entry in entries {
            if entry.entry_type == PkgEntryType::Folder {
                builder.add_directory(&entry.path_str);
            } else {
                let abs_path = entry
                    .abs_path
                    .as_ref()
                    .ok_or_else(|| "missing file path for PKG entry".to_string())?;
                let data = common::read_file_bytes(abs_path)?;
                builder.add_file(&entry.path_str, data);
                println!("Added: {} ({:#?})", entry.path_str, entry.entry_type);
            }
        }

        let output_file = common::create_output_file(output)?;
        let mut output_file = std::io::BufWriter::new(output_file);

        builder
            .write(&mut output_file)
            .map_err(|e| format!("failed to finalize PKG archive: {e}"))?;

        println!("PKG archive created successfully: {}", output.display());
        Ok(())
    }
}

#[derive(Args, Debug)]
pub struct PkgCreateArgs {
    /// Input folder path
    #[clap(short, long)]
    pub input: PathBuf,

    /// Output file path
    #[clap(short, long)]
    pub output: PathBuf,

    /// PKG content ID
    #[clap(long, default_value = "EP9000-RUST00005_00-RUST000000000001")]
    pub content_id: String,

    /// PKG title ID
    #[clap(long, default_value = "RUST00005")]
    pub title_id: String,

    /// PKG release type (debug, release)
    #[clap(long, default_value = "release")]
    pub release_type: String,

    /// PKG DRM type (free, local, network, pspgo, none)
    #[clap(long, default_value = "free")]
    pub drm_type: String,

    /// PKG platform (ps3, psp)
    #[clap(long, default_value = "ps3")]
    pub platform: String,

    /// PKG content type (game_data, game_exec, ps1_emu, psp_minis, system_update, psp_remaster, psp_neogeo, avatar, minis2, xmb_plugin, theme, disc_movie, widget, license_file, pspgo)
    #[clap(long, default_value = "game_exec")]
    pub content_type: String,
}

fn parse_release_type(value: &str) -> Result<PkgReleaseType, String> {
    match value.to_ascii_lowercase().as_str() {
        "debug" => Ok(PkgReleaseType::Debug),
        "release" => Ok(PkgReleaseType::Release),
        _ => Err(format!(
            "invalid release type: {value} (expected: debug, release)"
        )),
    }
}

fn parse_drm_type(value: &str) -> Result<PkgDrmType, String> {
    match value.to_ascii_lowercase().as_str() {
        "free" => Ok(PkgDrmType::Free),
        "local" => Ok(PkgDrmType::Local),
        "network" => Ok(PkgDrmType::Network),
        "pspgo" => Ok(PkgDrmType::PspGo),
        "none" => Ok(PkgDrmType::None),
        _ => Err(format!(
            "invalid DRM type: {value} (expected: free, local, network, pspgo, none)"
        )),
    }
}

fn parse_platform(value: &str) -> Result<PkgPlatform, String> {
    match value.to_ascii_lowercase().as_str() {
        "ps3" => Ok(PkgPlatform::PS3),
        "psp" => Ok(PkgPlatform::PSP),
        _ => Err(format!("invalid platform: {value} (expected: ps3, psp)")),
    }
}

fn parse_content_type(value: &str) -> Result<PkgContentType, String> {
    match value.to_ascii_lowercase().as_str() {
        "game_data" => Ok(PkgContentType::GameData),
        "game_exec" => Ok(PkgContentType::GameExec),
        "ps1_emu" => Ok(PkgContentType::Ps1Emu),
        "psp_minis" => Ok(PkgContentType::PspMinis),
        "system_update" => Ok(PkgContentType::SystemUpdate),
        "psp_remaster" => Ok(PkgContentType::PspRemaster),
        "psp_neogeo" => Ok(PkgContentType::PspNeoGeo),
        "avatar" => Ok(PkgContentType::Avatar),
        "minis2" => Ok(PkgContentType::Minis2),
        "xmb_plugin" => Ok(PkgContentType::XmbPlugin),
        "theme" => Ok(PkgContentType::Theme),
        "disc_movie" => Ok(PkgContentType::DiscMovie),
        "widget" => Ok(PkgContentType::Widget),
        "license_file" => Ok(PkgContentType::LicenseFile),
        "pspgo" => Ok(PkgContentType::PspGo),
        _ => Err(format!(
            "invalid content type: {value} (expected: game_data, game_exec, ps1_emu, psp_minis, system_update, psp_remaster, psp_neogeo, avatar, minis2, xmb_plugin, theme, disc_movie, widget, license_file, pspgo)"
        )),
    }
}
