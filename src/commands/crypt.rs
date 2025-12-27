use crate::commands::Execute;
use clap::Subcommand;

#[derive(Subcommand, Debug)]
pub enum Crypt {
    /// Encrypt a file
    Encrypt,
    /// Decrypt a file
    Decrypt,
}

impl Execute for Crypt {
    fn execute(self) {
        match self {
            Self::Encrypt => println!("Encrypting file..."),
            Self::Decrypt => println!("Decrypting file..."),
        }
    }
}
