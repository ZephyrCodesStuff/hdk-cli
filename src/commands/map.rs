use clap::Args;
use std::path::PathBuf;

use crate::commands::Execute;

use hdk_archive::mapper::Mapper;

const DEFAULT_OUTPUT_SUFFIX: &str = "mapped";

#[derive(Args, Debug)]
pub struct Map {
    /// Input directory to map
    #[clap(short, long)]
    pub input: PathBuf,

    /// (Optional) Output directory for mapped files
    ///
    /// If not provided, defaults to `./{input}.mapped`
    ///
    /// # Warning
    ///
    /// Some operating systems (such as macOS) do not allow for `.` in folder names.
    ///
    /// In such cases, the OS may automatically replace `.` with `_` or another character.
    #[clap(short, long)]
    pub output: Option<PathBuf>,

    /// (Optional) Whether to use the full set of regex patterns for mapping.
    ///
    /// This may increase accuracy but could slow down the mapping process.
    ///
    /// Defaults to `false`.
    #[clap(short, long, default_value_t = false)]
    pub full: bool,

    /// (Optional) UUID for mapping object archives.
    ///
    /// Objects **need** this UUID to be mapped correctly.
    ///
    /// Do **not** use for scenes.
    #[clap(short, long)]
    pub uuid: Option<String>,
}

impl Execute for Map {
    fn execute(self) {
        let mut mapper = Mapper::new(self.input.clone()).with_full(self.full);

        if let Some(uuid) = self.uuid {
            mapper = mapper.with_uuid(uuid);
        }

        let output_dir = self
            .output
            .unwrap_or_else(|| self.input.with_added_extension(DEFAULT_OUTPUT_SUFFIX));

        println!("Mapping files to: {}", output_dir.display());

        let result = mapper.run();

        println!("Mapped {} files.", result.mapped);

        if result.not_found.len() > 0 {
            println!("{} files could not be mapped:", result.not_found.len());
            for file in result.not_found {
                println!(" - {}", file.display());
            }
        }
    }
}
