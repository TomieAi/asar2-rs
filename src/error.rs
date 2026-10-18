// SPDX-License-Identifier: Apache-2.0 OR MIT
use std::{io::Error as IoError, path::PathBuf};
use thiserror::Error as ThisError;

#[derive(Debug, ThisError)]
pub enum Error {
	#[error("I/O error: {0}")]
	Io(#[from] IoError),
	#[error("I/O error while reading unpacked file '{path}': {err}")]
	UnpackedIoError { path: PathBuf, err: IoError },
	#[error("JSON error: {0}")]
	Json(#[from] serde_json::Error),
	#[error("Archive is truncated")]
	Truncated,
	#[error("Invalid asar header magic")]
	InvalidMagic,
	#[error("Hash mismatch in file '{file}'{}. Expected: {expected}, got: {actual}", .block.map(|b| format!(", block #{}", b)).unwrap_or_default())]
	HashMismatch {
		file: PathBuf,
		block: Option<usize>,
		expected: String,
		actual: String,
	},
	#[error("File '{0}' has already been written")]
	FileAlreadyWritten(PathBuf),
	#[error("Invalid hash algorithm: '{0}'")]
	InvalidHashAlgorithm(String),
}

pub type Result<T> = std::result::Result<T, Error>;
