// SPDX-License-Identifier: Apache-2.0 OR MIT
use crate::error::{Error, Result};
use byteorder::{LittleEndian, ReadBytesExt};
use serde::{de::Deserializer, Deserialize, Serialize};
use std::{
	collections::HashMap,
	fmt::{self, Display},
	path::PathBuf,
};

#[cfg(test)]
pub(crate) static TEST_ASAR: &[u8] = include_bytes!("../data/test.asar");

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HashAlgorithm {
	#[serde(rename = "SHA256")]
	Sha256,
}

impl Default for HashAlgorithm {
	fn default() -> Self {
		Self::Sha256
	}
}

impl Display for HashAlgorithm {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		match self {
			Self::Sha256 => write!(f, "SHA256"),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileIntegrity {
	pub algorithm: HashAlgorithm,
	pub hash: String,
	pub block_size: usize,
	#[serde(default)]
	pub blocks: Vec<String>,
}

impl FileIntegrity {
	pub fn new(algorithm: HashAlgorithm, hash: String, block_size: usize, blocks: Vec<String>) -> Self {
		Self {
			algorithm,
			hash,
			block_size,
			blocks,
		}
	}

	#[inline]
	pub const fn algorithm(&self) -> HashAlgorithm {
		self.algorithm
	}

	#[inline]
	pub fn hash(&self) -> &str {
		&self.hash
	}

	#[inline]
	pub const fn block_size(&self) -> usize {
		self.block_size
	}

	#[inline]
	pub fn blocks(&self) -> &[String] {
		&self.blocks
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct RawOffset(pub usize);

impl<'de> Deserialize<'de> for RawOffset {
	fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
	where
		D: Deserializer<'de>,
	{
		struct Visitor;
		impl<'de> serde::de::Visitor<'de> for Visitor {
			type Value = RawOffset;

			fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
				formatter.write_str("a string or integer offset")
			}

			fn visit_str<E>(self, v: &str) -> std::result::Result<Self::Value, E>
			where
				E: serde::de::Error,
			{
				v.parse::<usize>().map(RawOffset).map_err(E::custom)
			}

			fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
			where
				E: serde::de::Error,
			{
				Ok(RawOffset(v as usize))
			}

			fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
			where
				E: serde::de::Error,
			{
				if v >= 0 {
					Ok(RawOffset(v as usize))
				} else {
					Err(E::custom("negative offset"))
				}
			}
		}
		deserializer.deserialize_any(Visitor)
	}
}

/// Raw representation of an ASAR header node in single-pass JSON deserialization.
/// Unlike asar-rs which uses #[serde(untagged)] and backtracks repeatedly,
/// this struct deserializes in a single forward pass without backtracking.
#[derive(Debug, Clone, Deserialize)]
pub struct RawNode {
	pub files: Option<HashMap<String, RawNode>>,
	pub link: Option<String>,
	pub size: Option<usize>,
	pub offset: Option<RawOffset>,
	pub unpacked: Option<bool>,
	pub executable: Option<bool>,
	pub integrity: Option<FileIntegrity>,
}


const fn is_false(b: &bool) -> bool {
	!*b
}

const fn default_false() -> bool {
	false
}

fn serialize_offset<S>(offset: &usize, serializer: S) -> std::result::Result<S::Ok, S::Error>
where
	S: serde::Serializer,
{
	serializer.serialize_str(&offset.to_string())
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FileLocation {
	Offset {
		#[serde(serialize_with = "serialize_offset")]
		offset: usize,
	},
	Unpacked {
		#[serde(skip_serializing_if = "is_false", default = "default_false")]
		unpacked: bool,
	},
}

impl FileLocation {
	#[inline]
	pub const fn offset(offset: usize) -> Self {
		FileLocation::Offset { offset }
	}

	#[inline]
	pub const fn unpacked() -> Self {
		FileLocation::Unpacked { unpacked: true }
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct File {
	#[serde(flatten)]
	pub location: FileLocation,
	pub size: usize,
	#[serde(skip_serializing_if = "is_false", default = "default_false")]
	pub executable: bool,
	#[serde(skip_serializing_if = "Option::is_none")]
	pub integrity: Option<FileIntegrity>,
}

impl File {
	pub const fn new(
		location: FileLocation,
		size: usize,
		executable: bool,
		integrity: Option<FileIntegrity>,
	) -> Self {
		Self {
			location,
			size,
			executable,
			integrity,
		}
	}

	#[inline]
	pub const fn location(&self) -> FileLocation {
		self.location
	}

	#[inline]
	pub const fn offset(&self) -> Option<usize> {
		match self.location {
			FileLocation::Offset { offset } => Some(offset),
			_ => None,
		}
	}

	#[inline]
	pub const fn unpacked(&self) -> bool {
		matches!(self.location, FileLocation::Unpacked { .. })
	}

	#[inline]
	pub const fn size(&self) -> usize {
		self.size
	}

	#[inline]
	pub const fn executable(&self) -> bool {
		self.executable
	}

	#[inline]
	pub const fn integrity(&self) -> Option<&FileIntegrity> {
		self.integrity.as_ref()
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Header {
	File(File),
	Directory { files: HashMap<String, Self> },
	Link { link: PathBuf },
}

impl Header {
	pub fn new() -> Self {
		Self::Directory {
			files: HashMap::new(),
		}
	}

	/// Reads the raw node tree and the beginning offset of file payloads.
	/// Reads directly from byte slice with zero allocations for JSON data.
	pub fn read_from_slice(data: &[u8]) -> Result<(RawNode, usize)> {
		if data.len() < 16 {
			return Err(Error::Truncated);
		}
		let mut cursor = &data[..16];
		let _magic = cursor.read_u32::<LittleEndian>()?;
		let header_size = cursor.read_u32::<LittleEndian>()? as usize;
		let _size2 = cursor.read_u32::<LittleEndian>()?;
		let json_size = cursor.read_u32::<LittleEndian>()? as usize;

		if data.len() < 16 + json_size {
			return Err(Error::Truncated);
		}

		let json_slice = &data[16..16 + json_size];
		let raw: RawNode = serde_json::from_slice(json_slice)?;
		Ok((raw, header_size + 8))
	}

	/// Traditional Header reader for compatibility.
	pub fn read<Read: ReadBytesExt>(data: &mut Read) -> Result<(Self, usize)> {
		data.read_u32::<LittleEndian>()?;
		let header_size = data.read_u32::<LittleEndian>()? as usize;
		data.read_u32::<LittleEndian>()?;
		let json_size = data.read_u32::<LittleEndian>()? as usize;
		let mut bytes = vec![0_u8; json_size];
		data.read_exact(&mut bytes)?;
		let raw: RawNode = serde_json::from_slice(&bytes)?;
		let header = raw_node_to_header(raw);
		Ok((header, header_size + 8))
	}
}

impl Default for Header {
	fn default() -> Self {
		Self::new()
	}
}

fn raw_node_to_header(node: RawNode) -> Header {
	if let Some(link) = node.link {
		Header::Link { link: PathBuf::from(link) }
	} else if let Some(files) = node.files {
		let map = files.into_iter().map(|(k, v)| (k, raw_node_to_header(v))).collect();
		Header::Directory { files: map }
	} else {
		let size = node.size.unwrap_or(0);
		let location = if node.unpacked.unwrap_or(false) {
			FileLocation::Unpacked { unpacked: true }
		} else {
			FileLocation::Offset { offset: node.offset.map(|o| o.0).unwrap_or(0) }
		};
		Header::File(File {
			location,
			size,
			executable: node.executable.unwrap_or(false),
			integrity: node.integrity,
		})
	}
}
