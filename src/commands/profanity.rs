use std::{fs, path::Path};

use clap::Subcommand;
use hdk_archive::profanity::ProfanityDictionary;

use crate::commands::{Execute, IArg, IOArgs};

#[derive(Subcommand, Debug)]
pub enum Profanity {
    /// Extract / decrypt a binary profanity dictionary (.bin) into JSON format
    #[clap(alias = "x")]
    Extract(IOArgs),

    /// Build / encrypt a profanity dictionary from JSON format into binary (.bin)
    #[clap(alias = "c")]
    Build(IOArgs),

    /// Inspect a binary (.bin) or JSON profanity dictionary and print its contents
    #[clap(alias = "i")]
    Inspect(IArg),
}

impl Execute for Profanity {
    fn execute(self) {
        let result = match self {
            Self::Extract(args) => Self::extract(&args.input, &args.output),
            Self::Build(args) => Self::build(&args.input, &args.output),
            Self::Inspect(args) => Self::inspect(&args.input),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

impl Profanity {
    /// Extract a .bin dictionary to .json
    pub fn extract(input: &Path, output: &Path) -> Result<(), String> {
        println!(
            "Extracting profanity dictionary: {} -> {}",
            input.display(),
            output.display()
        );

        let dict = ProfanityDictionary::from_file(input)
            .map_err(|e| format!("failed to read and decrypt profanity dictionary: {e}"))?;

        let json_str = dict
            .to_json()
            .map_err(|e| format!("failed to serialize dictionary to JSON: {e}"))?;

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output directory: {e}"))?;
        }

        fs::write(output, json_str).map_err(|e| format!("failed to write JSON file: {e}"))?;

        let num_with_whitelist = dict
            .dictionary_items
            .iter()
            .filter(|i| !i.whitelist.is_empty())
            .count();

        println!("Successfully extracted profanity dictionary:");
        println!("  Version: {}", dict.version);
        println!("  Flags: 0x{:02X}", dict.flags);
        println!(
            "  Blacklisted Words: {} ({} have whitelist exceptions)",
            dict.dictionary_items.len(),
            num_with_whitelist
        );
        println!("  Conversion Items: {}", dict.convert_items.len());
        println!("  Custom Punctuation: {}", dict.custom_punctuation.len());
        println!("  Char Substitutions: {}", dict.char_substitutions.len());
        println!(
            "  Rev Char Substitutions: {}",
            dict.rev_char_substitutions.len()
        );

        Ok(())
    }

    /// Build a .json dictionary to .bin
    pub fn build(input: &Path, output: &Path) -> Result<(), String> {
        println!(
            "Building profanity dictionary: {} -> {}",
            input.display(),
            output.display()
        );

        let json_str =
            fs::read_to_string(input).map_err(|e| format!("failed to read JSON file: {e}"))?;

        let dict = ProfanityDictionary::from_json(&json_str)
            .map_err(|e| format!("failed to parse JSON profanity dictionary: {e}"))?;

        let bin_bytes = dict
            .to_bytes()
            .map_err(|e| format!("failed to build and encrypt binary dictionary: {e}"))?;

        if let Some(parent) = output.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create output directory: {e}"))?;
        }

        fs::write(output, &bin_bytes)
            .map_err(|e| format!("failed to write output binary file: {e}"))?;

        println!("Successfully built profanity dictionary:");
        println!("  Output: {} ({} bytes)", output.display(), bin_bytes.len());
        println!("  Blacklisted Words: {}", dict.dictionary_items.len());
        println!("  Conversion Items: {}", dict.convert_items.len());
        println!("  Char Substitutions: {}", dict.char_substitutions.len());
        println!(
            "  Rev Char Substitutions: {}",
            dict.rev_char_substitutions.len()
        );

        Ok(())
    }

    /// Inspect a .bin or .json dictionary
    pub fn inspect(input: &Path) -> Result<(), String> {
        let dict = if input.extension().is_some_and(|ext| ext == "json") {
            let json_str =
                fs::read_to_string(input).map_err(|e| format!("failed to read JSON file: {e}"))?;
            ProfanityDictionary::from_json(&json_str)
                .map_err(|e| format!("failed to parse JSON profanity dictionary: {e}"))?
        } else {
            ProfanityDictionary::from_file(input)
                .map_err(|e| format!("failed to read and decrypt profanity dictionary: {e}"))?
        };

        println!("============================================================");
        println!(" PlayStation Home Profanity Dictionary Info");
        println!(" File: {}", input.display());
        println!("============================================================");
        println!(" Version: {}", dict.version);
        println!(" Flags:   0x{:08X}", dict.flags);
        println!("------------------------------------------------------------");
        println!(" [Blacklisted Words: {}]", dict.dictionary_items.len());

        println!(
            " Words with Whitelist Exceptions: {}",
            dict.dictionary_items
                .iter()
                .filter(|i| !i.whitelist.is_empty())
                .count()
        );

        println!("\n Sample Blacklisted Words (first 10):");
        for (idx, item) in dict.dictionary_items.iter().take(10).enumerate() {
            if item.whitelist.is_empty() {
                println!("   {:2}. \"{}\"", idx + 1, item.word);
            } else {
                println!(
                    "   {:2}. \"{}\" (whitelist: {})",
                    idx + 1,
                    item.word,
                    item.whitelist.join(", ")
                );
            }
        }

        println!("\n [Conversion Rules: {}]", dict.convert_items.len());
        for (idx, item) in dict.convert_items.iter().take(5).enumerate() {
            let from_ch = char::from_u32(item.from).unwrap_or('?');
            let to_ch = char::from_u32(item.to).unwrap_or('?');
            println!(
                "   {:2}. U+{:04X} ('{}') -> U+{:04X} ('{}')",
                idx + 1,
                item.from,
                from_ch,
                item.to,
                to_ch
            );
        }
        if dict.convert_items.len() > 5 {
            println!("   ... ({} more rules)", dict.convert_items.len() - 5);
        }

        println!(
            "\n [Custom Punctuation Codepoints: {}]",
            dict.custom_punctuation.len()
        );
        let punct_preview: Vec<String> = dict
            .custom_punctuation
            .iter()
            .take(15)
            .map(|&p| {
                let ch = char::from_u32(p).unwrap_or('?');
                format!("'{}' (0x{:02X})", ch, p)
            })
            .collect();
        println!("   {}", punct_preview.join(", "));
        if dict.custom_punctuation.len() > 15 {
            println!(
                "   ... ({} more codepoints)",
                dict.custom_punctuation.len() - 15
            );
        }

        println!(
            "\n [Forward Character Substitutions: {}]",
            dict.char_substitutions.len()
        );
        for (idx, sub) in dict.char_substitutions.iter().take(5).enumerate() {
            let target_ch = char::from_u32(sub.target as u32).unwrap_or('?');
            let variants = sub
                .substitutions
                .iter()
                .map(|s| format!("\"{}\"", s))
                .collect::<Vec<_>>()
                .join(", ");
            println!("   {:2}. '{}' -> [{}]", idx + 1, target_ch, variants);
        }
        if dict.char_substitutions.len() > 5 {
            println!(
                "   ... ({} more mappings)",
                dict.char_substitutions.len() - 5
            );
        }

        println!(
            "\n [Reverse Character Substitutions: {}]",
            dict.rev_char_substitutions.len()
        );
        for (idx, rev) in dict.rev_char_substitutions.iter().take(5).enumerate() {
            let symbol_ch = char::from_u32(rev.symbol as u32).unwrap_or('?');
            let cands = rev
                .candidates
                .iter()
                .map(|&c| format!("'{}'", char::from_u32(c as u32).unwrap_or('?')))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "   {:2}. '{}' (0x{:04X}) -> [{}]",
                idx + 1,
                symbol_ch,
                rev.symbol,
                cands
            );
        }
        if dict.rev_char_substitutions.len() > 5 {
            println!(
                "   ... ({} more mappings)",
                dict.rev_char_substitutions.len() - 5
            );
        }

        println!("============================================================");

        Ok(())
    }
}
