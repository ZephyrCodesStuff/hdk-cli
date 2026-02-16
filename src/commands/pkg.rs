use clap::Subcommand;
use std::path::PathBuf;

use crate::commands::{Execute, IOArgs, Input};

#[derive(Subcommand, Debug)]
pub enum Pkg {
    /// Inspect a PlayStation 3 PKG file
    #[clap(alias = "i")]
    Inspect(Input),

    /// Extract contents of a PlayStation 3 PKG file
    #[clap(alias = "x")]
    Extract(IOArgs),
}

impl Execute for Pkg {
    fn execute(self) {
        let function = match self {
            Self::Inspect(args) => Pkg::inspect(&args.input),
            Self::Extract(args) => Pkg::extract(&args.input, &args.output),
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

    pub fn extract(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
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
}
