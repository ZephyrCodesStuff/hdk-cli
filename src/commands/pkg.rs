use clap::Subcommand;
use hdk_firmware::pkg::{PkgContentType, PkgDrmType, PkgPlatform, PkgReleaseType};
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
    Create(IOArgs),
}

impl Execute for Pkg {
    fn execute(self) {
        let function = match self {
            Self::Inspect(args) => Self::inspect(&args.input),
            Self::Extract(args) => Self::extract(&args.input, &args.output),
            Self::Create(args) => Self::create(&args.input, &args.output),
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

        for item in pkg.items().filter_map(|item| item.ok()) {
            let file_type = if item.entry.is_directory() {
                "Directory"
            } else {
                "File"
            };

            println!(
                "{} ({}), size: {} bytes",
                item.name, file_type, item.entry.data_size
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

    pub fn create(input: &Path, output: &Path) -> Result<(), String> {
        if !input.is_dir() {
            return Err(format!("input path {} is not a directory", input.display()));
        }

        let content_id = "EP9000-RUST00005_00-HOME000000000001";

        let mut builder = hdk_firmware::pkg::writer::PkgBuilder::new()
            .platform(PkgPlatform::PS3)
            .content_type(PkgContentType::GameExec)
            .release_type(PkgReleaseType::Release)
            .drm_type(PkgDrmType::Free)
            .content_id(content_id)
            .title_id("RUST00005");

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
            is_dir: bool,
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
                    is_dir: true,
                });
            } else if entry.file_type().is_file() {
                entries.push(PkgEntry {
                    path_str,
                    abs_path: Some(entry.path().to_path_buf()),
                    is_dir: false,
                });
            }
        }

        entries.sort_by(|a, b| a.path_str.cmp(&b.path_str));

        for entry in entries {
            if entry.is_dir {
                builder.add_directory(&entry.path_str);
            } else {
                let abs_path = entry
                    .abs_path
                    .as_ref()
                    .ok_or_else(|| "missing file path for PKG entry".to_string())?;
                let data = common::read_file_bytes(abs_path)?;
                builder.add_file(&entry.path_str, data);
                println!("Added: {}", entry.path_str);
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
