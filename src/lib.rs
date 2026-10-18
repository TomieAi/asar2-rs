// SPDX-License-Identifier: Apache-2.0 OR MIT
//! asar2: High-performance, zero-copy, flat-indexed asar archive reader and writer.
//! Replaces logarithmic tree traversals with O(1) direct lookups, instant directory checks,
//! and single-pass JSON header deserialization.

pub mod error;
pub mod header;
#[cfg(feature = "integrity")]
pub mod integrity;
pub mod reader;
#[cfg(feature = "write")]
pub mod writer;

pub use error::{Error, Result};
pub use header::{File, FileIntegrity, FileLocation, HashAlgorithm, Header, RawNode};
pub use reader::{normalize_str_path, AsarFile, AsarReader};
#[cfg(feature = "write")]
pub use writer::AsarWriter;

