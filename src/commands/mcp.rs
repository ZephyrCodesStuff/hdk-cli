use std::path::PathBuf;

use rmcp::{
    handler::server::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, TextContent},
    schemars::JsonSchema,
    tool, tool_handler, tool_router, ServerHandler,
    transport::io::stdio,
    ServiceExt,
};
use serde::Deserialize;

use crate::commands::{
    bar::Bar,
    compress::{self, Algorithm},
    crypt::{self, KnownFileType},
    pkg::{Pkg, PkgCreateArgs},
    profanity::Profanity,
    sceneid,
    sdat::Sdat,
    sharc::Sharc,
    ArchiveType, EndianArg,
};


#[derive(clap::Args, Debug)]
pub struct McpArgs {}

#[derive(Clone)]
pub struct HdkMcpServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

impl Default for HdkMcpServer {
    fn default() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

fn mcp_success(text: impl Into<String>) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::Text(TextContent::new(text.into()))])
}

fn mcp_error(text: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::Text(TextContent::new(text.into()))])
}

fn parse_endian(endian_str: Option<&str>) -> EndianArg {
    match endian_str.unwrap_or("big").to_ascii_lowercase().as_str() {
        "little" | "le" => EndianArg::Little,
        _ => EndianArg::Big,
    }
}

fn parse_archive_type(archive_type: Option<&str>) -> ArchiveType {
    match archive_type.unwrap_or("sharc").to_ascii_lowercase().as_str() {
        "bar" => ArchiveType::Bar,
        _ => ArchiveType::Sharc,
    }
}

fn parse_file_type(hint: Option<&str>) -> Option<KnownFileType> {
    match hint?.to_ascii_lowercase().as_str() {
        "odc" => Some(KnownFileType::Odc),
        "xml" => Some(KnownFileType::Xml),
        "scenelist" | "scene_list" => Some(KnownFileType::SceneList),
        "lua" => Some(KnownFileType::Lua),
        "bar" => Some(KnownFileType::Bar),
        "pem" => Some(KnownFileType::Pem),
        "hcdb" => Some(KnownFileType::Hcdb),
        "nav" => Some(KnownFileType::Nav),
        _ => None,
    }
}

fn parse_algorithm(alg: Option<&str>) -> Algorithm {
    match alg.unwrap_or("lzma").to_ascii_lowercase().as_str() {
        "zlib" => Algorithm::Zlib,
        _ => Algorithm::Lzma,
    }
}

// ── Parameter Structs ──────────────────────────────────────────────────────────

#[derive(Deserialize, JsonSchema)]
pub struct HdkSdatParams {
    #[schemars(description = "Action to perform: 'create', 'extract', or 'inspect'")]
    pub action: String,
    #[schemars(description = "Input path: directory to pack (for create) or SDAT file (for extract/inspect)")]
    pub input: String,
    #[schemars(description = "Output path: destination SDAT file (for create) or directory (for extract)")]
    pub output: Option<String>,
    #[schemars(description = "Archive type to wrap inside SDAT: 'sharc' or 'bar' (default: 'sharc')")]
    pub archive_type: Option<String>,
    #[schemars(description = "Endianness: 'big' or 'little' (default: 'big')")]
    pub endian: Option<String>,
    #[schemars(description = "Whether to protect the archive (default: false)")]
    pub protect: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkSharcParams {
    #[schemars(description = "Action to perform: 'create', 'extract', or 'inspect'")]
    pub action: String,
    #[schemars(description = "Input path: directory to pack (for create) or SHARC file (for extract/inspect)")]
    pub input: String,
    #[schemars(description = "Output path: destination SHARC file (for create) or directory (for extract)")]
    pub output: Option<String>,
    #[schemars(description = "Endianness: 'big' or 'little' (default: 'big')")]
    pub endian: Option<String>,
    #[schemars(description = "Whether to protect the archive (default: false)")]
    pub protect: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkBarParams {
    #[schemars(description = "Action to perform: 'create', 'extract', or 'inspect'")]
    pub action: String,
    #[schemars(description = "Input path: directory to pack (for create) or BAR file (for extract/inspect)")]
    pub input: String,
    #[schemars(description = "Output path: destination BAR file (for create) or directory (for extract)")]
    pub output: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkCryptParams {
    #[schemars(description = "Action to perform: 'encrypt', 'decrypt', or 'auto'")]
    pub action: String,
    #[schemars(description = "Input file path")]
    pub input: String,
    #[schemars(description = "Output file path (optional for 'auto')")]
    pub output: Option<String>,
    #[schemars(description = "Optional plaintext type hint for IV recovery: 'odc', 'xml', 'scenelist', 'lua', 'bar', 'pem', 'hcdb', 'nav'")]
    pub file_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkCompressParams {
    #[schemars(description = "Action to perform: 'compress' or 'decompress'")]
    pub action: String,
    #[schemars(description = "Input file path")]
    pub input: String,
    #[schemars(description = "Output file path")]
    pub output: String,
    #[schemars(description = "Compression algorithm: 'lzma' or 'zlib' (default: 'lzma')")]
    pub algorithm: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkMapParams {
    #[schemars(description = "Input directory containing extracted files to map")]
    pub input: String,
    #[schemars(description = "Output directory for mapped files (default: input + '.mapped')")]
    pub output: Option<String>,
    #[schemars(description = "Whether to use the full set of regex patterns (default: false)")]
    pub full: Option<bool>,
    #[schemars(description = "Optional UUID for mapping object archives (do not use for scenes)")]
    pub uuid: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkPkgParams {
    #[schemars(description = "Action to perform: 'inspect', 'extract', or 'create'")]
    pub action: String,
    #[schemars(description = "Input path: PKG file (for inspect/extract) or directory (for create)")]
    pub input: String,
    #[schemars(description = "Output path: extraction directory (for extract) or PKG file (for create)")]
    pub output: Option<String>,
    #[schemars(description = "PKG content ID (for create)")]
    pub content_id: Option<String>,
    #[schemars(description = "PKG title ID (for create)")]
    pub title_id: Option<String>,
    #[schemars(description = "PKG release type: 'debug' or 'release' (default: 'debug')")]
    pub release_type: Option<String>,
    #[schemars(description = "PKG DRM type: 'free', 'local', 'network', 'pspgo', 'none' (default: 'free')")]
    pub drm_type: Option<String>,
    #[schemars(description = "PKG platform: 'ps3' or 'psp' (default: 'ps3')")]
    pub platform: Option<String>,
    #[schemars(description = "PKG content type: 'game_data', 'game_exec', etc. (default: 'game_exec')")]
    pub content_type: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkProfanityParams {
    #[schemars(description = "Action to perform: 'extract' (.bin to .json), 'build' (.json to .bin), or 'inspect'")]
    pub action: String,
    #[schemars(description = "Input file path (.bin or .json)")]
    pub input: String,
    #[schemars(description = "Output file path (.json for extract, .bin for build)")]
    pub output: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct HdkSceneIdParams {
    #[schemars(description = "Action to perform: 'generate', 'forge', 'verify', or 'decode' / 'inspect'")]
    pub action: String,
    #[schemars(description = "Single SceneID UUID string to verify or decode")]
    pub id: Option<String>,
    #[schemars(description = "List of SceneID UUID strings to verify or decode")]
    pub ids: Option<Vec<String>>,
    #[schemars(description = "Target SceneID number (u16 as decimal or hex string, e.g. '0x1337' or '4919') for forge/generate")]
    pub target: Option<String>,
    #[schemars(description = "Target CRC16 checksum (u16 as decimal or hex string, e.g. '0xABCD') for forge/generate")]
    pub target_crc: Option<String>,
    #[schemars(description = "Number of SceneIDs to generate or forge (default: 1)")]
    pub count: Option<usize>,
    #[schemars(description = "Include verbose breakdown in output (default: false)")]
    pub verbose: Option<bool>,
}

// ── MCP Server Tool Router ───────────────────────────────────────────────────

#[tool_router]
impl HdkMcpServer {
    #[tool(description = "SDAT operations: create (pack dir to SDAT), extract (decrypt SDAT to dir), or inspect.")]
    async fn hdk_sdat(&self, Parameters(params): Parameters<HdkSdatParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        match params.action.to_ascii_lowercase().as_str() {
            "create" | "c" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("sdat"));
                let archive_type = parse_archive_type(params.archive_type.as_deref());
                let endian = parse_endian(params.endian.as_deref());
                let protect = params.protect.unwrap_or(false);

                match Sdat::create(&input_path, &output, archive_type, endian, protect) {
                    Ok(()) => Ok(mcp_success(format!("Successfully created SDAT archive at '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to create SDAT archive: {e}"))),
                }
            }
            "extract" | "x" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| {
                    let file_stem = input_path.file_stem().unwrap_or_default();
                    input_path.with_file_name(format!("{}_extracted", file_stem.to_string_lossy()))
                });

                match Sdat::extract(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully extracted SDAT to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to extract SDAT: {e}"))),
                }
            }
            "inspect" | "i" => match Sdat::inspect(&input_path) {
                Ok(info) => Ok(mcp_success(info)),
                Err(e) => Ok(mcp_error(format!("Failed to inspect SDAT: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: create, extract, inspect"))),
        }
    }

    #[tool(description = "SHARC operations: create (pack dir to SHARC), extract (extract SHARC to dir), or inspect.")]
    async fn hdk_sharc(&self, Parameters(params): Parameters<HdkSharcParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        match params.action.to_ascii_lowercase().as_str() {
            "create" | "c" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("sharc"));
                let endian = parse_endian(params.endian.as_deref());
                let protect = params.protect.unwrap_or(false);

                match Sharc::create(&input_path, &output, endian, protect) {
                    Ok(()) => Ok(mcp_success(format!("Successfully created SHARC archive at '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to create SHARC archive: {e}"))),
                }
            }
            "extract" | "x" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| {
                    let file_stem = input_path.file_stem().unwrap_or_default();
                    input_path.with_file_name(format!("{}_extracted", file_stem.to_string_lossy()))
                });

                match Sharc::extract(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully extracted SHARC to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to extract SHARC: {e}"))),
                }
            }
            "inspect" | "i" => match Sharc::inspect(&input_path) {
                Ok(info) => Ok(mcp_success(info)),
                Err(e) => Ok(mcp_error(format!("Failed to inspect SHARC: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: create, extract, inspect"))),
        }
    }

    #[tool(description = "BAR operations: create (pack dir to BAR), extract (extract BAR to dir), or inspect.")]
    async fn hdk_bar(&self, Parameters(params): Parameters<HdkBarParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        match params.action.to_ascii_lowercase().as_str() {
            "create" | "c" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("bar"));
                match Bar::create(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully created BAR archive at '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to create BAR archive: {e}"))),
                }
            }
            "extract" | "x" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| {
                    let file_stem = input_path.file_stem().unwrap_or_default();
                    input_path.with_file_name(format!("{}_extracted", file_stem.to_string_lossy()))
                });

                match Bar::extract(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully extracted BAR to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to extract BAR: {e}"))),
                }
            }
            "inspect" | "i" => match Bar::inspect(&input_path) {
                Ok(info) => Ok(mcp_success(info)),
                Err(e) => Ok(mcp_error(format!("Failed to inspect BAR: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: create, extract, inspect"))),
        }
    }

    #[tool(description = "Blowfish CTR cryptographic operations: encrypt, decrypt (via known-plaintext IV recovery), or auto-detect.")]
    async fn hdk_crypt(&self, Parameters(params): Parameters<HdkCryptParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        let hint = parse_file_type(params.file_type.as_deref());

        match params.action.to_ascii_lowercase().as_str() {
            "encrypt" | "e" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("enc"));
                match crypt::encrypt_file(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully encrypted '{}' to '{}'", input_path.display(), output.display()))),
                    Err(e) => Ok(mcp_error(format!("Encryption failed: {e}"))),
                }
            }
            "decrypt" | "d" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("dec"));
                match crypt::decrypt_file(&input_path, &output, hint) {
                    Ok(()) => Ok(mcp_success(format!("Successfully decrypted '{}' to '{}'", input_path.display(), output.display()))),
                    Err(e) => Ok(mcp_error(format!("Decryption failed: {e}"))),
                }
            }
            "auto" | "a" => match crypt::auto_crypt(&input_path, hint) {
                Ok(()) => Ok(mcp_success(format!("Auto crypt completed for '{}'", input_path.display()))),
                Err(e) => Ok(mcp_error(format!("Auto crypt failed: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: encrypt, decrypt, auto"))),
        }
    }

    #[tool(description = "Segmented compression: compress or decompress files using EdgeLZMA or EdgeZLib.")]
    async fn hdk_compress(&self, Parameters(params): Parameters<HdkCompressParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        let output_path = PathBuf::from(&params.output);
        let algorithm = parse_algorithm(params.algorithm.as_deref());

        match params.action.to_ascii_lowercase().as_str() {
            "compress" | "c" => match compress::compress(&input_path, &output_path, algorithm) {
                Ok(()) => Ok(mcp_success(format!("Successfully compressed '{}' to '{}' ({:?})", input_path.display(), output_path.display(), algorithm))),
                Err(e) => Ok(mcp_error(format!("Compression failed: {e}"))),
            },
            "decompress" | "d" => match compress::decompress(&input_path, &output_path, algorithm) {
                Ok(()) => Ok(mcp_success(format!("Successfully decompressed '{}' to '{}' ({:?})", input_path.display(), output_path.display(), algorithm))),
                Err(e) => Ok(mcp_error(format!("Decompression failed: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: compress, decompress"))),
        }
    }

    #[tool(description = "Map hashed/unknown files to original directory and filename structure using HDK mapper.")]
    async fn hdk_map(&self, Parameters(params): Parameters<HdkMapParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);
        let output_path = params.output.map(PathBuf::from).unwrap_or_else(|| {
            let mut p = input_path.clone();
            p.set_extension("mapped");
            p
        });

        let mut mapper = hdk_archive::mapper::Mapper::new(input_path.clone())
            .with_output_folder(output_path.clone())
            .with_full(params.full.unwrap_or(false));

        if let Some(uuid) = params.uuid {
            mapper = mapper.with_uuid(uuid);
        }

        let result = mapper.run();
        Ok(mcp_success(format!(
            "Mapping completed for '{}' -> '{}'\n- Mapped files: {}\n- Unmapped files: {}",
            input_path.display(),
            output_path.display(),
            result.mapped,
            result.not_found.len()
        )))
    }

    #[tool(description = "PlayStation 3 PKG operations: inspect, extract, or create PKG files.")]
    async fn hdk_pkg(&self, Parameters(params): Parameters<HdkPkgParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);

        match params.action.to_ascii_lowercase().as_str() {
            "inspect" | "i" => match Pkg::inspect(&input_path) {
                Ok(info) => Ok(mcp_success(info)),
                Err(e) => Ok(mcp_error(format!("Failed to inspect PKG: {e}"))),
            },
            "extract" | "x" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| {
                    let file_stem = input_path.file_stem().unwrap_or_default();
                    input_path.with_file_name(format!("{}_extracted", file_stem.to_string_lossy()))
                });

                match Pkg::extract(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully extracted PKG to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to extract PKG: {e}"))),
                }
            }
            "create" | "c" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("pkg"));
                let args = PkgCreateArgs {
                    input: input_path,
                    output,
                    content_id: params.content_id.unwrap_or_else(|| "EP9000-RUST00005_00-RUST000000000001".to_string()),
                    title_id: params.title_id.unwrap_or_else(|| "RUST00005".to_string()),
                    release_type: params.release_type.unwrap_or_else(|| "debug".to_string()),
                    drm_type: params.drm_type.unwrap_or_else(|| "free".to_string()),
                    platform: params.platform.unwrap_or_else(|| "ps3".to_string()),
                    content_type: params.content_type.unwrap_or_else(|| "game_exec".to_string()),
                };

                match Pkg::create(&args) {
                    Ok(()) => Ok(mcp_success(format!("Successfully created PKG file at '{}'", args.output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to create PKG: {e}"))),
                }
            }
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: inspect, extract, create"))),
        }
    }

    #[tool(description = "Profanity Dictionary operations: extract (.bin to .json), build (.json to .bin), or inspect.")]
    async fn hdk_profanity(&self, Parameters(params): Parameters<HdkProfanityParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let input_path = PathBuf::from(&params.input);

        match params.action.to_ascii_lowercase().as_str() {
            "extract" | "x" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("json"));
                match Profanity::extract(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully extracted profanity dictionary to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to extract profanity dictionary: {e}"))),
                }
            }
            "build" | "c" => {
                let output = params.output.map(PathBuf::from).unwrap_or_else(|| input_path.with_extension("bin"));
                match Profanity::build(&input_path, &output) {
                    Ok(()) => Ok(mcp_success(format!("Successfully built profanity dictionary to '{}'", output.display()))),
                    Err(e) => Ok(mcp_error(format!("Failed to build profanity dictionary: {e}"))),
                }
            }
            "inspect" | "i" => match Profanity::inspect(&input_path) {
                Ok(info) => Ok(mcp_success(info)),
                Err(e) => Ok(mcp_error(format!("Failed to inspect profanity dictionary: {e}"))),
            },
            other => Ok(mcp_error(format!("Unknown action '{other}'. Supported: extract, build, inspect"))),
        }
    }

    #[tool(description = "PlayStation Home SceneID operations: generate, forge, verify, and decode/inspect SceneIDs.")]
    async fn hdk_sceneid(&self, Parameters(params): Parameters<HdkSceneIdParams>) -> Result<CallToolResult, rmcp::ErrorData> {
        let count = params.count.unwrap_or(1);
        let verbose = params.verbose.unwrap_or(false);

        match params.action.to_ascii_lowercase().as_str() {
            "generate" | "g" | "new" | "n" | "create" | "c" => {
                let target = match params.target.as_deref() {
                    Some(t) => match sceneid::parse_u16(t) {
                        Ok(val) => Some(val),
                        Err(e) => return Ok(mcp_error(format!("Failed to parse target: {e}"))),
                    },
                    None => None,
                };
                let crc = match params.target_crc.as_deref() {
                    Some(c) => match sceneid::parse_u16(c) {
                        Ok(val) => Some(val),
                        Err(e) => return Ok(mcp_error(format!("Failed to parse target CRC: {e}"))),
                    },
                    None => None,
                };

                let results = sceneid::SceneId::generate_ids(count, target, crc);
                let output = if verbose {
                    results
                        .iter()
                        .enumerate()
                        .map(|(i, d)| {
                            if results.len() > 1 {
                                format!("--- SceneID [{}/{}] ---\n{}", i + 1, results.len(), d.format_inspection())
                            } else {
                                d.format_inspection()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    results
                        .iter()
                        .map(|d| format!("{} (Scene ID: 0x{:04X} / {}, CRC16: 0x{:04X})", d.uuid, d.extracted_id, d.extracted_id, d.given_crc16))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(mcp_success(output))
            }
            "forge" | "f" => {
                let target = match params.target.as_deref() {
                    Some(t) => match sceneid::parse_u16(t) {
                        Ok(val) => val,
                        Err(e) => return Ok(mcp_error(format!("Failed to parse target: {e}"))),
                    },
                    None => return Ok(mcp_error("Target SceneID number is required for forge action (e.g. '0x1337' or '4919')")),
                };
                let crc = match params.target_crc.as_deref() {
                    Some(c) => match sceneid::parse_u16(c) {
                        Ok(val) => Some(val),
                        Err(e) => return Ok(mcp_error(format!("Failed to parse target CRC: {e}"))),
                    },
                    None => None,
                };

                let results = sceneid::SceneId::generate_ids(count, Some(target), crc);
                let output = if verbose {
                    results
                        .iter()
                        .enumerate()
                        .map(|(i, d)| {
                            if results.len() > 1 {
                                format!("--- Forged SceneID [{}/{}] ---\n{}", i + 1, results.len(), d.format_inspection())
                            } else {
                                d.format_inspection()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    results
                        .iter()
                        .map(|d| format!("{} (Scene ID: 0x{:04X} / {}, CRC16: 0x{:04X})", d.uuid, d.extracted_id, d.extracted_id, d.given_crc16))
                        .collect::<Vec<_>>()
                        .join("\n")
                };

                Ok(mcp_success(output))
            }
            "verify" | "v" | "check" => {
                let mut ids = Vec::new();
                if let Some(id) = params.id {
                    ids.push(id);
                }
                if let Some(extra_ids) = params.ids {
                    ids.extend(extra_ids);
                }

                if ids.is_empty() {
                    return Ok(mcp_error("No SceneID UUID(s) provided. Specify 'id' or 'ids'."));
                }

                let mut out = Vec::new();
                let mut all_valid = true;

                for id_str in &ids {
                    match sceneid::SceneIdDetails::from_str(id_str) {
                        Ok(details) => {
                            if details.is_valid {
                                out.push(format!(
                                    "✓ {}: VALID (Scene ID: 0x{:04X} / {}, CRC16: 0x{:04X})",
                                    details.uuid, details.extracted_id, details.extracted_id, details.given_crc16
                                ));
                            } else {
                                all_valid = false;
                                out.push(format!(
                                    "✗ {}: INVALID CRC16 (Given: 0x{:04X}, Expected: 0x{:04X}, Extracted Scene ID: 0x{:04X} / {})",
                                    details.uuid, details.given_crc16, details.expected_crc16, details.extracted_id, details.extracted_id
                                ));
                            }
                        }
                        Err(e) => {
                            all_valid = false;
                            out.push(format!("✗ {id_str}: INVALID ({e})"));
                        }
                    }
                }

                if ids.len() > 1 {
                    let valid_count = ids.len() - out.iter().filter(|s| s.starts_with('✗')).count();
                    out.push(format!("\nSummary: {valid_count}/{} valid SceneIDs.", ids.len()));
                }

                if all_valid {
                    Ok(mcp_success(out.join("\n")))
                } else {
                    Ok(mcp_error(out.join("\n")))
                }
            }
            "decode" | "d" | "inspect" | "i" | "extract" | "x" => {
                let mut ids = Vec::new();
                if let Some(id) = params.id {
                    ids.push(id);
                }
                if let Some(extra_ids) = params.ids {
                    ids.extend(extra_ids);
                }

                if ids.is_empty() {
                    return Ok(mcp_error("No SceneID UUID(s) provided. Specify 'id' or 'ids'."));
                }

                let mut out = Vec::new();
                for (i, id_str) in ids.iter().enumerate() {
                    if ids.len() > 1 {
                        out.push(format!("--- SceneID [{}/{}] ---", i + 1, ids.len()));
                    }
                    match sceneid::SceneIdDetails::from_str(id_str) {
                        Ok(details) => {
                            out.push(details.format_inspection());
                        }
                        Err(e) => {
                            out.push(format!("Error decoding '{id_str}': {e}"));
                        }
                    }
                }

                Ok(mcp_success(out.join("\n")))
            }
            other => Ok(mcp_error(format!(
                "Unknown action '{other}'. Supported: generate, forge, verify, decode, inspect"
            ))),
        }
    }
}

#[tool_handler]
impl ServerHandler for HdkMcpServer {}

pub async fn run(_args: McpArgs) {
    let server = HdkMcpServer::default();
    match server.serve(stdio()).await {
        Ok(running) => {
            if let Err(e) = running.waiting().await {
                eprintln!("MCP server exited with error: {:?}", e);
            }
        }
        Err(e) => {
            eprintln!("Failed to start MCP server: {:?}", e);
        }
    }
}
