<div align="center">

  <h1>hdk-cli</h1>

  <p>
    <strong>A command-line interface for the <a href="https://github.com/ZephyrCodesStuff/hdk-rs">hdk-rs</a> PlayStation Home development toolkit.</strong>
  </p>

  <p>
    <a href="https://github.com/ZephyrCodesStuff/hdk-cli/actions"><img src="https://img.shields.io/github/actions/workflow/status/ZephyrCodesStuff/hdk-cli/clippy.yml?branch=main&style=flat-square" alt="Build Status"></a>
    <a href="#license"><img src="https://img.shields.io/badge/license-AGPLv3-blue?style=flat-square" alt="License"></a>
  </p>

</div>

---

## 🌟 Authors

- [@zeph](https://github.com/ZephyrCodesStuff) (that's me!)

### Acknowledgements

- [@I-Knight-I](https://github.com/I-Knight-I) for their massive help with the cryptographic implementations, the compression algorithms and other miscellaneous bits of knowledge
- [@AgentDark447](https://github.com/GitHubProUser67) for their open-source software, allowing me to learn about the SHARC archive format
- @hykem for their efforts in reverse engineering the PS3 file formats such as NPD and SCE

## 📖 Overview

**hdk-cli** is the companion command-line tool for [`hdk-rs`](https://github.com/ZephyrCodesStuff/hdk-rs). It exposes the full power of the library's format support directly from the terminal, making it easy to inspect, pack, unpack, encrypt, and compress PlayStation Home and PS3 files without writing any code.

> ⚠️ **Status: Work In Progress** — This tool is under active development. Commands and flags may change as the underlying library stabilises.

## ⚡ Performance

`hdk` and `hdk-cli` are currently **the most performant tools available for working with PlayStation Home** file formats, often by a *large* margin.

An insane amount of time has been spent optimizing the core algorithms in `hdk-rs`, and `hdk-cli`, anywhere from using `smallvec` to avoid heap-allocations for small files, to parallelizing work across multiple threads with `rayon`, to using `memmap2` for zero-copy file access when possible.

The sample file for benchmarking is a 1.86 retail `COREDATA.SHARC` archive, which contains 1442 entries and is fully compressed and encrypted.

Benchmarks currently show these best results:

- **SHARC creation**: compression takes **~0.6s** with ISA-L support, or **~2.5s** without (using `cloudflare-zlib`).
- **SHARC extraction**: decompressing COREDATA.SHARC (1442 entries, fully compressed and encrypted) takes **~0.4s**.
- **Reading the file**: reading and parsing COREDATA.SHARC with memory-mapping takes **~900 microseconds**; without memory-mapping it takes **~350 microseconds**.
- **Mapping**: TBD; not yet measured.

> ⚠️ **ISA-L support in CI**: only Linux currently has ISA-L support in CI, so Windows and macOS binaries are built without it. If you want to build with ISA-L support on those platforms, you will need to clone the repository and run `cargo build --release --all-features` yourself.

## 🔧 Commands

The binary is invoked as `hdk`. All sub-commands support `--help` for usage details.

### `sdat` — SDAT / SDATA archives

| Sub-command    | Alias | Description                                |
| :------------- | :---: | :----------------------------------------- |
| `sdat create`  |  `c`  | Pack a directory into a Sony SDATA archive |
| `sdat extract` |  `x`  | Unpack an SDATA archive to a directory     |

> ⚠️ **Warning**: This tool writes *all* archive timestamps as **big-endian**, for one simple reason: they are easier to patch when working in a hex-editor manually.
>
> Should you use the `.time` files it produces for anything else, make sure to parse them as **big-endian**!

### `sharc` — SHARC archives

| Sub-command     | Alias | Description                                            |
| :-------------- | :---: | :----------------------------------------------------- |
| `sharc create`  |  `c`  | Pack a directory into a PlayStation Home SHARC archive |
| `sharc extract` |  `x`  | Unpack a SHARC archive to a directory                  |

### `bar` — BAR archives

| Sub-command   | Alias | Description                                                      |
| :------------ | :---: | :--------------------------------------------------------------- |
| `bar create`  |  `c`  | Pack a directory into a BAR archive (entries are XTEA-encrypted) |
| `bar extract` |  `x`  | Unpack a BAR archive to a directory                              |

> **Tip:** For `create`, place a 4-byte little-endian `.time` file in the input directory to embed a specific archive timestamp.

### `crypt` — Blowfish CTR encryption

| Sub-command     | Alias | Description                                                                            |
| :-------------- | :---: | :------------------------------------------------------------------------------------- |
| `crypt encrypt` |  `e`  | Encrypt a file with Blowfish CTR                                                       |
| `crypt decrypt` |  `d`  | Decrypt a file using known-plaintext IV recovery                                       |
| `crypt auto`    |  `a`  | Auto-detect whether the file is encrypted or decrypted and perform the opposite action |

`decrypt` and `auto` accept an optional `--type` / `-t` flag to hint the expected plaintext format, which guides IV recovery. Supported types:

| Value        | Description                                |
| :----------- | :----------------------------------------- |
| `odc`        | ODC / SDC XML (UTF-8 BOM)                  |
| `xml`        | Raw XML (`<?xml`)                          |
| `scene-list` | Scene list XML (`<SCENELI`)                |
| `lua`        | Lua script (`LoadLibr`)                    |
| `bar`        | BAR archive magic                          |
| `pem`        | PEM certificate                            |
| `hcdb`       | HCDB database (brute-forced segment count) |

If `--type` is omitted, all known types are tried automatically.

### `compress` — EdgeZLib / EdgeLZMA compression

| Sub-command           | Alias | Description                                            |
| :-------------------- | :---: | :----------------------------------------------------- |
| `compress compress`   |  `c`  | Compress a file using EdgeZLib or EdgeLZMA             |
| `compress decompress` |  `d`  | Decompress a file compressed with EdgeZLib or EdgeLZMA |

Both commands accept `-a` / `--algorithm` with values `lzma` (default) or `zlib`.

### `map` — Path mapper

Recover original file paths from a directory of hashed-name archive entries.

```
hdk map --input <dir> [--output <dir>] [--full] [--uuid <uuid>]
```

| Flag              | Description                                                            |
| :---------------- | :--------------------------------------------------------------------- |
| `--input` / `-i`  | Directory of extracted, hash-named files                               |
| `--output` / `-o` | Output directory (defaults to `<input>.mapped`)                        |
| `--full` / `-f`   | Use the full regex pattern set for higher accuracy (slower)            |
| `--uuid` / `-u`   | UUID for object archives (required for objects; do not use for scenes) |

### `pkg` — PlayStation 3 PKG files

| Sub-command   | Alias | Description                                          |
| :------------ | :---: | :--------------------------------------------------- |
| `pkg inspect` |  `i`  | Print PKG header, metadata packets, and file listing |
| `pkg extract` |  `x`  | Extract the contents of a PKG file to a directory    |
| `pkg create`  |  `c`  | Build a PKG file from a directory                    |

## 💿 Building

```sh
# Clone the repository
git clone https://github.com/ZephyrCodesStuff/hdk-cli
cd hdk-cli

# Build a release binary
cargo build --release

# The binary will be at target/release/hdk
./target/release/hdk --help
```

## 💛 Contributions

Contributions are welcome! Since this project aims for stability and correctness:

1. Please ensure `cargo clippy` passes.
2. Do not go out-of-scope. Your PR should only touch what is relevant to your addition.
3. Make sure your PR contains all the details needed to know what you're changing and why.

Note: although not strictly enforced, running `clippy::pedantic` every now and then is not a bad idea.

## 📄 License

This project is licensed under the **GNU Affero General Public License v3.0 (AGPL-3.0)**.

**What this means:**

- ✅ **You can** use this tool to build open source workflows.
- ✅ **You can** modify the tool to suit your needs.
- 🛑 **If you distribute** a modified binary, you **must** provide the corresponding source code.

See [LICENSE](LICENSE) for more details.
