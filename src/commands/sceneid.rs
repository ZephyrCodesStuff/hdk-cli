use std::{
    fs,
    path::{Path, PathBuf},
};

use clap::{Args, Subcommand};
use hdk_secure::sceneid::SceneID;
use uuid::Uuid;

use crate::commands::Execute;

/// XOR mask used in Home's SceneID algorithm
pub const UUID_XOR: [u8; 16] = [
    0xB9, 0x20, 0x86, 0xBC, 0x3E, 0x8B, 0x4A, 0xDF, 0xA3, 0x01, 0x4D, 0xEE, 0x2F, 0xA3, 0xAB, 0x69,
];

/// Scatter table used in Home's SceneID algorithm
pub const SCATTER_TABLE: [[u8; 2]; 16] = [
    [3, 12],
    [8, 6],
    [2, 8],
    [4, 5],
    [5, 1],
    [4, 10],
    [1, 3],
    [11, 5],
    [3, 4],
    [5, 6],
    [13, 10],
    [7, 5],
    [2, 9],
    [3, 9],
    [10, 8],
    [4, 10],
];

/// Parses a 16-bit integer from decimal or hex format (e.g. `0x1337` or `4919`).
pub fn parse_u16(s: &str) -> Result<u16, String> {
    let s = s.trim();
    if let Some(hex_str) = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")) {
        u16::from_str_radix(hex_str, 16).map_err(|e| format!("Invalid hex value '{s}': {e}"))
    } else if s.chars().all(|c| c.is_ascii_digit()) {
        s.parse::<u16>().map_err(|e| format!("Invalid decimal value '{s}': {e}"))
    } else if s.chars().all(|c| c.is_ascii_hexdigit()) {
        u16::from_str_radix(s, 16).map_err(|e| format!("Invalid hex value '{s}': {e}"))
    } else {
        Err(format!("Could not parse '{s}' as u16 (expected decimal or 0x-prefixed hex)"))
    }
}

/// Detailed information decoded from a SceneID UUID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SceneIdDetails {
    pub uuid: Uuid,
    pub src_bytes: [u8; 14],
    pub given_crc16: u16,
    pub expected_crc16: u16,
    pub is_valid: bool,
    pub extracted_id: u16,
    pub scatter_index: usize,
    pub scatter_positions: (u8, u8),
}

impl SceneIdDetails {
    /// Decodes a UUID string or parses bytes to produce full SceneID details.
    pub fn from_str(id_str: &str) -> Result<Self, String> {
        let uuid = Uuid::parse_str(id_str.trim()).map_err(|e| format!("Invalid UUID format '{id_str}': {e}"))?;
        Ok(Self::from_uuid(uuid))
    }

    /// Decodes a parsed UUID to produce full SceneID details.
    pub fn from_uuid(uuid: Uuid) -> Self {
        let bytes = uuid.as_bytes();
        let mut src_bytes = [0u8; 14];
        src_bytes.copy_from_slice(&bytes[0..14]);

        let given_crc16 = u16::from_le_bytes(bytes[14..16].try_into().unwrap());

        let mut calculated_crc = crc16::State::<crc16::AUG_CCITT>::new();
        calculated_crc.update(&src_bytes);
        let expected_crc16 = calculated_crc.get();

        let is_valid = given_crc16 == expected_crc16;

        let mut xor_bytes = [0u8; 16];
        for (i, (a, b)) in bytes.iter().zip(UUID_XOR).enumerate() {
            xor_bytes[i] = a ^ b;
        }

        let scatter_index = (xor_bytes[0] & 15) as usize;
        let pos1 = SCATTER_TABLE[scatter_index][0];
        let pos2 = SCATTER_TABLE[scatter_index][1];

        let extracted_id = (xor_bytes[pos1 as usize] as u16) | ((xor_bytes[pos2 as usize] as u16) << 8);

        Self {
            uuid,
            src_bytes,
            given_crc16,
            expected_crc16,
            is_valid,
            extracted_id,
            scatter_index,
            scatter_positions: (pos1, pos2),
        }
    }

    /// Formatted multi-line inspection breakdown.
    pub fn format_inspection(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("UUID:                {}\n", self.uuid));
        out.push_str(&format!(
            "Extracted Scene ID:  0x{:04X} ({})\n",
            self.extracted_id, self.extracted_id
        ));
        if self.is_valid {
            out.push_str(&format!("CRC16:               0x{:04X} (Valid)\n", self.given_crc16));
        } else {
            out.push_str(&format!(
                "CRC16:               0x{:04X} (INVALID - Expected: 0x{:04X})\n",
                self.given_crc16, self.expected_crc16
            ));
        }
        out.push_str(&format!(
            "Scatter Lookup:      Index {} -> Positions [{}, {}]\n",
            self.scatter_index, self.scatter_positions.0, self.scatter_positions.1
        ));
        out.push_str(&format!(
            "Source Bytes (0..14): {}\n",
            self.src_bytes
                .iter()
                .map(|b| format!("{b:02X}"))
                .collect::<Vec<_>>()
                .join(" ")
        ));
        out.push_str(&format!(
            "CRC Bytes (14..16):   {:02X} {:02X}\n",
            self.uuid.as_bytes()[14],
            self.uuid.as_bytes()[15]
        ));
        out
    }
}

#[derive(Args, Debug)]
pub struct GenerateArgs {
    /// Target Scene ID number to forge (e.g. `0x1337` or `4919`). If omitted, random valid IDs are generated.
    #[clap(short, long, value_parser = parse_u16)]
    pub target: Option<u16>,

    /// Target CRC16 checksum (e.g. `0xABCD`). Only used when --target is specified.
    #[clap(short, long, value_parser = parse_u16)]
    pub crc: Option<u16>,

    /// Number of Scene IDs to generate
    #[clap(short = 'n', long, default_value_t = 1)]
    pub count: usize,

    /// Print detailed information about generated IDs
    #[clap(short, long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct ForgeArgs {
    /// Target Scene ID number (e.g. `0x1337` or `4919`)
    #[clap(value_parser = parse_u16)]
    pub target: u16,

    /// Specific target CRC16 checksum (e.g. `0xABCD`). If omitted, a random valid CRC is generated.
    #[clap(short, long, value_parser = parse_u16)]
    pub crc: Option<u16>,

    /// Number of Scene IDs to forge
    #[clap(short = 'n', long, default_value_t = 1)]
    pub count: usize,

    /// Print detailed information about forged IDs
    #[clap(short, long, default_value_t = false)]
    pub verbose: bool,
}

#[derive(Args, Debug)]
pub struct VerifyArgs {
    /// SceneID UUID string(s) to verify
    #[clap(num_args = 0..)]
    pub ids: Vec<String>,

    /// Path to a file containing SceneID UUIDs to verify (one per line)
    #[clap(short, long)]
    pub file: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct DecodeArgs {
    /// SceneID UUID string(s) to decode / inspect
    #[clap(num_args = 0..)]
    pub ids: Vec<String>,

    /// Path to a file containing SceneID UUIDs to decode (one per line)
    #[clap(short, long)]
    pub file: Option<PathBuf>,
}

/// PlayStation Home SceneID operations: generate, forge, verify, and decode/inspect.
#[derive(Subcommand, Debug)]
pub enum SceneId {
    /// Generate one or more valid SceneIDs (optionally mapped to a target number)
    #[clap(alias = "g", alias = "new", alias = "n", alias = "create", alias = "c")]
    Generate(GenerateArgs),

    /// Forge one or more SceneIDs that map to a specific target number
    #[clap(alias = "f")]
    Forge(ForgeArgs),

    /// Verify the CRC16 validity and format of SceneID UUIDs
    #[clap(alias = "v", alias = "check")]
    Verify(VerifyArgs),

    /// Decode SceneID UUIDs and inspect extracted ID numbers, CRC, and scatter positions
    #[clap(alias = "d", alias = "inspect", alias = "i", alias = "extract", alias = "x")]
    Decode(DecodeArgs),
}

impl Execute for SceneId {
    fn execute(self) {
        let result = match self {
            Self::Generate(args) => Self::execute_generate(args),
            Self::Forge(args) => Self::execute_forge(args),
            Self::Verify(args) => Self::execute_verify(args),
            Self::Decode(args) => Self::execute_decode(args),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

impl SceneId {
    /// Read IDs from argument list and optional file.
    pub fn collect_ids(ids: &[String], file: Option<&Path>) -> Result<Vec<String>, String> {
        let mut collected = ids.to_vec();

        if let Some(path) = file {
            let content = fs::read_to_string(path)
                .map_err(|e| format!("Failed to read IDs file '{}': {e}", path.display()))?;

            for line in content.lines() {
                let trimmed = line.trim();
                if !trimmed.is_empty() && !trimmed.starts_with('#') {
                    collected.push(trimmed.to_string());
                }
            }
        }

        if collected.is_empty() {
            return Err("No SceneID UUIDs provided. Specify IDs on the command line or use --file <FILE>.".to_string());
        }

        Ok(collected)
    }

    /// Generates SceneIDs according to `GenerateArgs`.
    pub fn generate_ids(
        count: usize,
        target: Option<u16>,
        crc: Option<u16>,
    ) -> Vec<SceneIdDetails> {
        let mut results = Vec::with_capacity(count);

        for _ in 0..count {
            let scene_id = match target {
                Some(t) => SceneID::forge(t, crc),
                None => SceneID::new(),
            };
            results.push(SceneIdDetails::from_uuid(scene_id.final_id));
        }

        results
    }

    fn execute_generate(args: GenerateArgs) -> Result<(), String> {
        if args.count == 0 {
            return Err("Count must be at least 1".to_string());
        }

        let results = Self::generate_ids(args.count, args.target, args.crc);

        for (i, details) in results.iter().enumerate() {
            if args.verbose {
                if results.len() > 1 {
                    println!("--- SceneID [{}/{}] ---", i + 1, results.len());
                }
                print!("{}", details.format_inspection());
            } else {
                println!("{}", details.uuid);
            }
        }

        Ok(())
    }

    fn execute_forge(args: ForgeArgs) -> Result<(), String> {
        if args.count == 0 {
            return Err("Count must be at least 1".to_string());
        }

        let results = Self::generate_ids(args.count, Some(args.target), args.crc);

        for (i, details) in results.iter().enumerate() {
            if args.verbose {
                if results.len() > 1 {
                    println!("--- Forged SceneID [{}/{}] ---", i + 1, results.len());
                }
                print!("{}", details.format_inspection());
            } else {
                println!("{}", details.uuid);
            }
        }

        Ok(())
    }

    fn execute_verify(args: VerifyArgs) -> Result<(), String> {
        let ids = Self::collect_ids(&args.ids, args.file.as_deref())?;
        let mut all_valid = true;
        let mut valid_count = 0;
        let total = ids.len();

        for id_str in &ids {
            match SceneIdDetails::from_str(id_str) {
                Ok(details) => {
                    if details.is_valid {
                        valid_count += 1;
                        println!(
                            "✓ {}: VALID (Scene ID: 0x{:04X} / {}, CRC16: 0x{:04X})",
                            details.uuid, details.extracted_id, details.extracted_id, details.given_crc16
                        );
                    } else {
                        all_valid = false;
                        println!(
                            "✗ {}: INVALID CRC16 (Given: 0x{:04X}, Expected: 0x{:04X}, Extracted Scene ID: 0x{:04X} / {})",
                            details.uuid, details.given_crc16, details.expected_crc16, details.extracted_id, details.extracted_id
                        );
                    }
                }
                Err(e) => {
                    all_valid = false;
                    println!("✗ {id_str}: INVALID ({e})");
                }
            }
        }

        if total > 1 {
            println!("\nSummary: {valid_count}/{total} valid SceneIDs.");
        }

        if !all_valid {
            return Err("One or more SceneIDs failed verification.".to_string());
        }

        Ok(())
    }

    fn execute_decode(args: DecodeArgs) -> Result<(), String> {
        let ids = Self::collect_ids(&args.ids, args.file.as_deref())?;

        for (i, id_str) in ids.iter().enumerate() {
            if ids.len() > 1 {
                println!("--- SceneID [{}/{}] ---", i + 1, ids.len());
            }

            match SceneIdDetails::from_str(id_str) {
                Ok(details) => {
                    print!("{}", details.format_inspection());
                }
                Err(e) => {
                    eprintln!("Error decoding '{id_str}': {e}");
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_u16() {
        assert_eq!(parse_u16("0x1337").unwrap(), 0x1337);
        assert_eq!(parse_u16("0XABCD").unwrap(), 0xABCD);
        assert_eq!(parse_u16("4919").unwrap(), 4919);
        assert_eq!(parse_u16("0").unwrap(), 0);
        assert_eq!(parse_u16("65535").unwrap(), 65535);
        assert_eq!(parse_u16("  0x42  ").unwrap(), 0x42);
        assert!(parse_u16("0x10000").is_err());
        assert!(parse_u16("invalid").is_err());
        assert!(parse_u16("-1").is_err());
    }

    #[test]
    fn test_generate_random() {
        let generated = SceneId::generate_ids(5, None, None);
        assert_eq!(generated.len(), 5);
        for id in generated {
            assert!(id.is_valid);
            assert_eq!(id.given_crc16, id.expected_crc16);
            let verified = SceneID::verify_str(&id.uuid.to_string()).expect("Should verify successfully");
            assert_eq!(verified.extract_scene_id(), id.extracted_id);
        }
    }

    #[test]
    fn test_generate_and_forge_with_target() {
        let target: u16 = 0x1337;
        let forged = SceneId::generate_ids(3, Some(target), None);
        assert_eq!(forged.len(), 3);
        for id in forged {
            assert!(id.is_valid);
            assert_eq!(id.extracted_id, target);
            assert_eq!(id.given_crc16, id.expected_crc16);
            let verified = SceneID::verify_str(&id.uuid.to_string()).expect("Should verify successfully");
            assert_eq!(verified.extract_scene_id(), target);
        }
    }

    #[test]
    fn test_forge_with_target_and_crc() {
        let target: u16 = 0xBEEF;
        let target_crc: u16 = 0x1234;
        let forged = SceneId::generate_ids(1, Some(target), Some(target_crc));
        assert_eq!(forged.len(), 1);
        let id = &forged[0];
        assert!(id.is_valid);
        assert_eq!(id.extracted_id, target);
        assert_eq!(id.given_crc16, target_crc);
        assert_eq!(id.expected_crc16, target_crc);
    }

    #[test]
    fn test_scene_id_details_from_str() {
        let scene_id = SceneID::forge(0x4242, None);
        let details = SceneIdDetails::from_str(&scene_id.final_id.to_string()).unwrap();
        assert!(details.is_valid);
        assert_eq!(details.extracted_id, 0x4242);
        assert_eq!(details.given_crc16, details.expected_crc16);

        let inspection = details.format_inspection();
        assert!(inspection.contains("UUID:"));
        assert!(inspection.contains("0x4242 (16962)"));
        assert!(inspection.contains("(Valid)"));
    }

    #[test]
    fn test_scene_id_corrupted_crc() {
        let scene_id = SceneID::forge(0x1337, None);
        // Corrupt first byte of source data
        let mut bytes = *scene_id.final_id.as_bytes();
        bytes[0] ^= 0xFF;
        let corrupted_uuid = Uuid::from_bytes(bytes);

        let details = SceneIdDetails::from_uuid(corrupted_uuid);
        assert!(!details.is_valid);
        assert_ne!(details.given_crc16, details.expected_crc16);

        let inspection = details.format_inspection();
        assert!(inspection.contains("(INVALID - Expected:"));
    }

    #[test]
    fn test_scene_id_invalid_uuid_string() {
        assert!(SceneIdDetails::from_str("not-a-valid-uuid").is_err());
    }
}
