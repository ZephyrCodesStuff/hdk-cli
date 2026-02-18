// Used for `.with_added_extension()` in `src/commands/map.rs`
#![feature(path_add_extension)]

use clap::Parser;

mod commands;
mod keys;
mod magic;

use crate::commands::Execute;

fn main() {
    let args = commands::Main::parse();
    args.command.execute();
}
