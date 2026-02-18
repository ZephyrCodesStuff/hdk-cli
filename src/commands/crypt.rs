use std::path::PathBuf;

use crate::{
    commands::{Execute, IOArgs},
    magic::MimeType,
};
use clap::{Args, Subcommand, ValueEnum};
use ctr::cipher::KeyIvInit;
use hdk_secure::{
    modes::{BlowfishEcb, BlowfishEcbDec, BlowfishPS3},
    reader::CryptoReader,
};

#[derive(Args, Debug)]
pub struct DecryptArgs {
    #[clap(flatten)]
    pub io: IOArgs,

    /// Hint the expected plaintext file type for the known-plaintext IV recovery.
    ///
    /// If omitted, all known types are tried automatically.
    #[clap(short = 't', long = "type", value_enum)]
    pub file_type: Option<KnownFileType>,
}

#[derive(Args, Debug)]
pub struct AutoArgs {
    /// Input file path (will be decrypted or encrypted in-place, writing to a .dec / .enc sibling)
    #[clap(short, long)]
    pub input: PathBuf,

    /// Hint the expected plaintext file type for the known-plaintext IV recovery.
    ///
    /// If omitted, all known types are tried automatically.
    #[clap(short = 't', long = "type", value_enum)]
    pub file_type: Option<KnownFileType>,
}

/// Known plaintext file types whose first 8 bytes are well-defined.
///
/// These are used for the known-plaintext attack to recover the Blowfish CTR IV.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum KnownFileType {
    /// ODC / SDC XML (UTF-8 BOM + `<?xm`)
    Odc,
    /// Raw `<?xml ve` XML
    Xml,
    /// `<SCENELI` scene list XML
    SceneList,
    /// `LoadLibr` Lua script
    Lua,
    /// BAR archive (`0xE1 0x17 0xEF 0xAD ...`)
    Bar,
    /// PEM certificate (`-----BEG`)
    Pem,
    /// HCDB database (`segs` + `0x01 0x05` + 2-byte segment count — brute-forced)
    Hcdb,
}

impl KnownFileType {
    /// Returns all variants for brute-force iteration.
    pub fn all() -> &'static [KnownFileType] {
        &[
            KnownFileType::Odc,
            KnownFileType::Xml,
            KnownFileType::SceneList,
            KnownFileType::Lua,
            KnownFileType::Bar,
            KnownFileType::Pem,
            KnownFileType::Hcdb,
        ]
    }

    /// The known first 8 plaintext bytes for this file type.
    ///
    /// Returns `None` for types that require brute-forcing part of the header
    /// (e.g. [`KnownFileType::Hcdb`]).
    pub fn known_plaintext(&self) -> Option<[u8; 8]> {
        match self {
            KnownFileType::Odc => Some([0xEF, 0xBB, 0xBF, 0x3C, 0x3F, 0x78, 0x6D, 0x6C]),
            KnownFileType::Xml => Some(*b"<?xml ve"),
            KnownFileType::SceneList => Some(*b"<SCENELI"),
            KnownFileType::Lua => Some(*b"LoadLibr"),
            KnownFileType::Bar => Some([0xE1, 0x17, 0xEF, 0xAD, 0x00, 0x00, 0x00, 0x01]),
            KnownFileType::Pem => Some(*b"-----BEG"),
            // HCDB has a 2-byte segment count at bytes 6-7 that is unknown — use brute_force_hcdb_iv instead.
            KnownFileType::Hcdb => None,
        }
    }
}

#[derive(Subcommand, Debug)]
pub enum Crypt {
    /// Encrypt a file
    #[clap(alias = "e")]
    Encrypt(IOArgs),
    /// Decrypt a file using known-plaintext IV recovery
    #[clap(alias = "d")]
    Decrypt(DecryptArgs),
    /// Automatic mode: detects if the file is encrypted or decrypted and performs the opposite action
    ///
    /// This is a really magical way to use the CLI!
    #[clap(alias = "a")]
    Auto(AutoArgs),
}

impl Execute for Crypt {
    fn execute(self) {
        let result = match self {
            Self::Encrypt(ref args) => encrypt_file(&args.input, &args.output),
            Self::Decrypt(ref args) => {
                decrypt_file(&args.io.input, &args.io.output, args.file_type)
            }
            Self::Auto(ref args) => auto_crypt(&args.input, args.file_type),
        };

        if let Err(e) = result {
            eprintln!("Error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Heuristic helpers
// ---------------------------------------------------------------------------

/// Status of the file based on entropy / magic analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Heuristic {
    Encrypted(HeuristicReason),
    Decrypted(HeuristicReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HeuristicReason {
    /// Recognizable magic bytes were found.
    MagicBytes(MimeType),
    /// Shannon entropy is above the threshold.
    HighEntropy,
    /// Shannon entropy is below the threshold.
    LowEntropy,
}

/// Minimum Shannon entropy (bits/byte) to treat data as encrypted.
const ENTROPY_THRESHOLD: f32 = 7.5;

/// Minimum entropy drop (bits/byte) after decryption to consider it successful.
///
/// Unsuccessful decryption will cause change within ~0.1 bits/byte due to noise shuffling,
/// so this is a pretty safe threshold.
const ENTROPY_DROP_THRESHOLD: f32 = 1.0;

fn status_heuristic(data: &[u8]) -> Heuristic {
    let matcher = crate::magic::get_matcher();

    if let Some(t) = matcher.get(data) {
        return Heuristic::Decrypted(HeuristicReason::MagicBytes((t.mime_type(), t.extension())));
    }

    let entropy = entropy::shannon_entropy(data) as f32;
    if entropy > ENTROPY_THRESHOLD {
        Heuristic::Encrypted(HeuristicReason::HighEntropy)
    } else {
        Heuristic::Decrypted(HeuristicReason::LowEntropy)
    }
}

// ---------------------------------------------------------------------------
// Known-plaintext IV recovery
// ---------------------------------------------------------------------------

/// Recover the Blowfish-CTR IV from the ciphertext using a known-plaintext attack.
///
/// Blowfish-CTR (PS3 variant) produces its first keystream block by ECB-encrypting
/// the IV.  Since `ciphertext[0..8] = plaintext[0..8] XOR ECB(IV)`, we can recover
/// `ECB(IV)` by XOR-ing, then ECB-*decrypt* to get the original IV.
fn recover_iv(
    key: &[u8; 32],
    ciphertext: &[u8],
    known_plaintext: &[u8; 8],
) -> Result<[u8; 8], String> {
    if ciphertext.len() < 8 {
        return Err("Ciphertext too short for IV recovery".to_string());
    }

    // Step 1: XOR known plaintext against the first ciphertext block → ECB(IV)
    let mut ecb_iv = [0u8; 8];
    for i in 0..8 {
        ecb_iv[i] = known_plaintext[i] ^ ciphertext[i];
    }

    // Step 2: ECB-decrypt the block to get the raw IV.
    //
    // `BlowfishEcbDec` is `ecb::Decryptor<Blowfish>` and operates on 8-byte blocks.
    use ctr::cipher::{BlockDecryptMut, KeyInit, block_padding::NoPadding};
    let ecb_cipher = BlowfishEcbDec::new_from_slice(key)
        .map_err(|e| format!("Failed to create ECB cipher: {e}"))?;

    let mut block = ecb_iv;
    ecb_cipher
        .decrypt_padded_mut::<NoPadding>(&mut block)
        .map_err(|e| format!("ECB decrypt failed: {e}"))?;

    Ok(block)
}

// ---------------------------------------------------------------------------
// HCDB brute-force IV recovery
// ---------------------------------------------------------------------------

/// The known prefix of an HCDB plaintext header (bytes 0-5).
///
/// Layout: `b"segs"` (4 bytes) + version `0x01 0x05` (2 bytes) + segment count (2 bytes, unknown).
const HCDB_KNOWN_PREFIX: [u8; 6] = [b's', b'e', b'g', b's', 0x01, 0x05];

/// Maximum plausible segment count to search for HCDB brute-force.
const HCDB_MAX_SEGMENTS: u16 = u16::MAX;

/// Brute-force the 2-byte HCDB segment count to recover the Blowfish-CTR IV.
///
/// HCDB plaintext header bytes 0-7 are: `segs 0x01 0x05 <count_hi> <count_lo>`.
/// The segment count occupies bytes 6-7 and is unknown, so we try all 65536 values.
///
/// Verification oracle: decrypt the first 16 bytes with each candidate IV and check
/// that the `u32` at plaintext bytes 12-15 (big-endian) equals the total file size.
fn brute_force_hcdb_iv(key: &[u8; 32], ciphertext: &[u8]) -> Result<(u16, [u8; 8]), String> {
    if ciphertext.len() < 16 {
        return Err("HCDB ciphertext too short (need at least 16 bytes)".to_string());
    }

    let file_size = ciphertext.len() as u32;

    // We need to CTR-decrypt only the first 16 bytes for each candidate.
    // CTR keystream: block 0 = ECB(IV), block 1 = ECB(IV+1).
    // Decrypt those two Blowfish ECB blocks once per candidate.
    use ctr::cipher::{BlockDecryptMut, BlockEncryptMut, KeyInit, block_padding::NoPadding};

    for seg_count in 0u16..=HCDB_MAX_SEGMENTS {
        // Build the 8-byte known-plaintext candidate.
        let mut known = [0u8; 8];
        known[..6].copy_from_slice(&HCDB_KNOWN_PREFIX);
        known[6..8].copy_from_slice(&seg_count.to_be_bytes());

        // Step 1: XOR with ciphertext[0..8] to get ECB_k(IV).
        let mut ecb_iv = [0u8; 8];
        for i in 0..8 {
            ecb_iv[i] = known[i] ^ ciphertext[i];
        }

        // Step 2: ECB-decrypt to recover the raw IV.
        let ecb_dec = BlowfishEcbDec::new_from_slice(key)
            .map_err(|e| format!("ECB cipher init failed: {e}"))?;
        let mut iv_candidate = ecb_iv;
        ecb_dec
            .decrypt_padded_mut::<NoPadding>(&mut iv_candidate)
            .map_err(|e| format!("ECB decrypt failed: {e}"))?;

        // Step 3: CTR-decrypt the first 16 bytes using the candidate IV.
        //
        // CTR keystream bytes 0-15 = ECB_k(IV) || ECB_k(IV+1).
        // We already have ECB_k(IV) = ecb_iv (before the ECB-decrypt step above).
        // Compute ECB_k(IV+1) on the fly.
        let iv_plus_one = (u64::from_be_bytes(iv_candidate).wrapping_add(1)).to_be_bytes();
        let ecb_enc =
            BlowfishEcb::new_from_slice(key).map_err(|e| format!("ECB enc init failed: {e}"))?;
        let mut keystream_block1 = iv_plus_one;
        ecb_enc
            .encrypt_padded_mut::<NoPadding>(&mut keystream_block1, 8)
            .map_err(|e| format!("ECB encrypt failed: {e}"))?;

        let mut plain16 = [0u8; 16];
        for i in 0..8 {
            plain16[i] = ciphertext[i] ^ ecb_iv[i];
            plain16[i + 8] = ciphertext[i + 8] ^ keystream_block1[i];
        }

        // Step 4: Oracle — bytes 12-15 of HCDB plaintext are the file size (BE u32).
        let size_field = u32::from_be_bytes(plain16[12..16].try_into().unwrap());
        if size_field == file_size {
            eprintln!(
                "  [Hcdb] found segment count = {seg_count}, IV = {:02x?}",
                iv_candidate
            );
            return Ok((seg_count, iv_candidate));
        }
    }

    Err("HCDB brute-force exhausted all segment counts without a match".to_string())
}

/// CTR-decrypt `data` in-place using the given key and IV.
fn ctr_decrypt_inplace(key: &[u8; 32], iv: &[u8; 8], data: &mut Vec<u8>) -> Result<(), String> {
    use std::io::Read;

    let cipher = BlowfishPS3::new(key.into(), iv.into());
    let mut cursor = std::io::Cursor::new(data.as_slice());
    let mut reader = CryptoReader::new(&mut cursor, cipher);

    let mut decrypted = Vec::with_capacity(data.len());
    reader
        .read_to_end(&mut decrypted)
        .map_err(|e| format!("CTR decrypt failed: {e}"))?;

    *data = decrypted;
    Ok(())
}

// ---------------------------------------------------------------------------
// Public commands
// ---------------------------------------------------------------------------

/// Encrypt `input` → `output`.
///
/// The IV is derived from the SHA-1 hash of the plaintext (first 8 bytes of the digest).
pub fn encrypt_file(input: &PathBuf, output: &PathBuf) -> Result<(), String> {
    use std::io::Read;

    let data =
        std::fs::read(input).map_err(|e| format!("Failed to read file for encryption: {e}"))?;

    // Derive IV from SHA-1 of the plaintext.
    let mut hasher = sha1_smol::Sha1::new();
    hasher.update(&data);
    let digest = hasher.digest().bytes();

    let iv: [u8; 8] = digest[..8].try_into().unwrap();
    println!("IV (from SHA-1): {:02x?}", iv);

    let cipher = BlowfishPS3::new(&crate::keys::BLOWFISH_DEFAULT_KEY.into(), &iv.into());
    let mut cursor = std::io::Cursor::new(data.as_slice());
    let mut reader = CryptoReader::new(&mut cursor, cipher);

    let mut encrypted = Vec::with_capacity(data.len());
    reader
        .read_to_end(&mut encrypted)
        .map_err(|e| format!("Encryption failed: {e}"))?;

    std::fs::write(output, &encrypted)
        .map_err(|e| format!("Failed to write encrypted file: {e}"))?;

    println!("Encrypted → {}", output.display());
    Ok(())
}

/// Decrypt `input` → `output` using a known-plaintext attack to recover the IV.
///
/// If `hint` is given, only that plaintext header is tried.
/// Otherwise every [`KnownFileType`] is attempted and the first that produces
/// recognizable output is used.
pub fn decrypt_file(
    input: &PathBuf,
    output: &PathBuf,
    hint: Option<KnownFileType>,
) -> Result<(), String> {
    let data =
        std::fs::read(input).map_err(|e| format!("Failed to read file for decryption: {e}"))?;

    let key = &crate::keys::BLOWFISH_DEFAULT_KEY;

    let candidates: &[KnownFileType] = hint
        .as_ref()
        .map(std::slice::from_ref)
        .unwrap_or_else(|| KnownFileType::all());

    for file_type in candidates {
        // HCDB has an unknown 2-byte segment count in its header, so we brute-force
        // all 65536 values and use a size-field oracle rather than the generic KPA path.
        let (iv, verified_by_oracle) = if *file_type == KnownFileType::Hcdb {
            eprintln!("  [Hcdb] brute-forcing segment count (0..=65535)…");
            match brute_force_hcdb_iv(key, &data) {
                // The brute-force already confirmed correctness via the file-size oracle,
                // so we can skip the entropy check for this type.
                Ok((_seg_count, iv)) => (iv, true),
                Err(e) => {
                    eprintln!("  [Hcdb] brute-force failed: {e}");
                    continue;
                }
            }
        } else {
            let known = match file_type.known_plaintext() {
                Some(k) => k,
                None => continue, // should not happen for non-Hcdb types
            };
            match recover_iv(key, &data, &known) {
                Ok(iv) => (iv, false),
                Err(e) => {
                    eprintln!("  [{file_type:?}] IV recovery failed: {e}");
                    continue;
                }
            }
        };

        let mut attempt = data.clone();
        if let Err(e) = ctr_decrypt_inplace(key, &iv, &mut attempt) {
            eprintln!("  [{file_type:?}] CTR decrypt failed: {e}");
            continue;
        }

        // HCDB: the brute-force oracle already confirmed the IV is correct (it matched
        // the file-size field), so skip entropy checking — HCDB bodies are EdgeLZMA-
        // compressed and will still read as high-entropy after decryption.
        let success = if verified_by_oracle {
            println!(
                "Decrypted as {file_type:?} (validated by file-size oracle), IV: {:02x?}",
                iv
            );
            true
        } else {
            // Verification: we CANNOT use magic bytes here because the KPA forces
            // the first 8 bytes of every attempt to equal `known_plaintext` — so
            // magic would always match regardless of whether the IV was correct.
            //
            // Instead we compare the entropy of the body (bytes 8..) before and
            // after decryption.  A genuine decryption causes a clear entropy drop;
            // a wrong candidate just shuffles noise into different noise.
            let body_start = 8.min(data.len());
            let entropy_before = entropy::shannon_entropy(&data[body_start..]) as f32;
            let entropy_after = entropy::shannon_entropy(&attempt[body_start..]) as f32;
            let drop = entropy_before - entropy_after;

            eprintln!(
                "  [{file_type:?}] entropy {entropy_before:.3} → {entropy_after:.3} (drop {drop:.3})"
            );

            if drop >= ENTROPY_DROP_THRESHOLD {
                println!(
                    "Decrypted as {file_type:?} (entropy drop {drop:.3}), IV: {:02x?}",
                    iv
                );
                true
            } else {
                false
            }
        };

        if success {
            std::fs::write(output, &attempt)
                .map_err(|e| format!("Failed to write decrypted file: {e}"))?;
            println!("Decrypted → {}", output.display());
            return Ok(());
        }
        // Not a match — try the next candidate.
    }

    Err(format!(
        "Could not decrypt '{}': none of the known-plaintext candidates produced recognizable output.\n\
         Try specifying --type explicitly.",
        input.display()
    ))
}

/// Auto mode: detect whether the file is encrypted or decrypted, then do the reverse.
pub fn auto_crypt(input: &PathBuf, hint: Option<KnownFileType>) -> Result<(), String> {
    let data = std::fs::read(input).map_err(|e| format!("Failed to read file: {e}"))?;

    match status_heuristic(&data) {
        Heuristic::Decrypted(reason) => {
            println!("File appears decrypted ({reason:?}) — encrypting…");
            // Place output next to input with a `.enc` extension.
            let output = input.with_extension(
                format!(
                    "{}.enc",
                    input.extension().and_then(|e| e.to_str()).unwrap_or("")
                )
                .trim_start_matches('.'),
            );
            encrypt_file(input, &output)
        }
        Heuristic::Encrypted(reason) => {
            println!("File appears encrypted ({reason:?}) — decrypting…");
            // Place output next to input with a `.dec` extension.
            let output = input.with_extension(
                format!(
                    "{}.dec",
                    input.extension().and_then(|e| e.to_str()).unwrap_or("")
                )
                .trim_start_matches('.'),
            );
            decrypt_file(input, &output, hint)
        }
    }
}
