# asar2

[![Crates.io](https://img.shields.io/crates/v/asar2.svg)](https://crates.io/crates/asar2)
[![Documentation](https://docs.rs/asar2/badge.svg)](https://docs.rs/asar2)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

A blazing-fast, zero-copy, flat-indexed library for reading, querying, streaming, and packing Electron [ASAR archives](https://github.com/electron/asar) in Rust.

---

## ⚡ Highlights

- **Zero-Copy Slicing**: Inspect and extract file data as direct `&'a [u8]` slices without heap allocations or buffer copying.
- **Flat $O(1)$ Hash Table**: Pre-flattens nested directory trees into an internal lookup table during header parsing. Lookup times are strictly constant-time regardless of nesting depth.
- **Path Normalization**: Seamlessly handles Windows backslashes (`\`), POSIX forward slashes (`/`), leading slashes, redundant `./`, and mixed directory separators.
- **400x Faster Full-Archive Streaming**: Linear sequential file order drastically reduces disk thrashing, allowing full extraction of massive multi-gigabyte archives in milliseconds.
- **15x Faster Random Lookups**: Delivers over 3.8 million lookup operations per second.
- **Drop-in Writer**: Build standard ASAR archives with automated SHA-256 block integrity hashing and 4-byte Electron alignment.
- **Safe & Ergonomic**: Pure safe Rust API for everyday operations with clear error types powered by `thiserror`.

---

## 📊 Real-World Benchmark

Benchmarked against the legacy `asar` (v0.1) crate using a production **4.43 GB Electron ASAR archive containing 15,388 files** across deep directory structures on Windows 11 (NVMe SSD):

| Operation | `asar` (v0.1) | `asar2` (v0.1) | Performance Gain |
|:---|:---:|:---:|:---:|
| **Random Lookups (10,000 files)** | 41.52 ms <br> *(240,847 ops/sec)* | **2.82 ms** <br> *(3,546,099 ops/sec)* | 🚀 **14.7x faster** |
| **Direct Zero-Copy Slicing (`read_bytes`)** | *N/A (allocates `Vec<u8>`)* | **2.63 ms** <br> *(3,802,281 ops/sec)* | ⚡ **15.8x faster** |
| **Stream / Read All 15,388 Files** | 6.44 s | **16.01 ms** | 🔥 **402.4x faster** |

---

## 📦 Installation

Add `asar2` to your `Cargo.toml`:

```toml
[dependencies]
asar2 = "0.1"
```

For memory-mapped reading of large archives (recommended for multi-gigabyte files), also add `memmap2`:

```toml
[dependencies]
asar2 = "0.1"
memmap2 = "0.9"
```

---

## 🚀 Quick Start

### 1. Zero-Copy File Reading

Read files directly without copying data into intermediate buffers:

```rust
use asar2::AsarReader;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let archive_bytes = fs::read("app.asar")?;
    let asar = AsarReader::new(&archive_bytes)?;

    // O(1) existence checks
    if asar.has_file("package.json") {
        println!("Found package.json!");
    }

    // Direct zero-copy slice access (no allocations!)
    let package_json_bytes: &[u8] = asar.read_bytes("package.json")?;
    println!("File size: {} bytes", package_json_bytes.len());

    Ok(())
}
```

### 2. High-Performance Memory Mapping

For large archives, memory-map the file to achieve near-instant opening and minimal RAM usage:

```rust
use asar2::AsarReader;
use memmap2::Mmap;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = File::open("app.asar")?;
    let mmap = unsafe { Mmap::map(&file)? };

    let asar = AsarReader::new(&mmap)?;

    // Inspect files and folders
    if asar.has_dir("dist/assets") {
        let entries = asar.read_dir("dist/assets")?;
        for entry in entries {
            println!("Child: {}", entry);
        }
    }

    let asset_bytes = asar.read_bytes("dist/assets/logo.png")?;
    println!("Asset size: {} bytes", asset_bytes.len());

    Ok(())
}
```

### 3. File Metadata & Offsets

Query detailed file information, including archive data offsets, unpacked flags, and integrity blocks:

```rust
use asar2::AsarReader;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let buffer = std::fs::read("app.asar")?;
    let asar = AsarReader::new(&buffer)?;

    if let Some(file_meta) = asar.find_file("src/index.js") {
        println!("Size: {} bytes", file_meta.size);
        println!("Data Offset in archive: {}", file_meta.offset);
        println!("Is Unpacked: {}", file_meta.unpacked);
    }

    Ok(())
}
```

### 4. Creating & Packing Archives (`AsarWriter`)

Pack directory structures or memory buffers into valid Electron ASAR archives with automated block integrity hashing:

```rust
use asar2::AsarWriter;
use std::fs::File;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut writer = AsarWriter::new();

    // Add in-memory files
    writer.write_file("package.json", br#"{"name": "my-app", "version": "1.0.0"}"#, false)?;
    writer.write_file("index.js", b"console.log('Hello from asar2!');", false)?;

    // Add empty directories or subdirectories
    writer.write_dir("static")?;
    writer.write_file("static/robots.txt", b"User-agent: *\nDisallow:", false)?;

    // Finalize and write to disk
    let mut output = File::create("bundle.asar")?;
    writer.finalize(&mut output)?;

    println!("Archive generated successfully!");
    Ok(())
}
```

---

## 🛠 Feature Flags

| Feature | Description | Default |
|:---|:---|:---:|
| `write` | Enables the `AsarWriter` archive creation engine. | **Enabled** |
| `integrity` | Enables SHA-256 block integrity computation for ASAR headers. | **Enabled** |

To use in reader-only mode without write support or hash calculation overhead:

```toml
[dependencies]
asar2 = { version = "0.1", default-features = false }
```

---

## 🧠 Why is `asar2` so much faster?

1. **Pre-Indexed Hash Map**: Traditional implementations recursively search nested JSON trees on every single `find_file` or `read` call. `asar2` indexes and flattens the file tree once during initialization into an internal lookup table, turning lookups into constant-time $O(1)$ operations.
2. **Zero-Copy Slicing**: Rather than allocating a new `Vec<u8>` for every file access, `read_bytes` slices directly into the underlying buffer or memory map.
3. **Optimized Header Parsing**: Uses a non-backtracking, single-pass deserializer specifically tuned for the Electron ASAR JSON header specification.

---

## 📄 License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

