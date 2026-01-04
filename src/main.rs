use clap::Parser;

mod commands;
mod keys;

use crate::commands::Execute;

fn main() {
    let args = commands::Main::parse();
    args.command.execute();
}
