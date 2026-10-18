// SPDX-License-Identifier: Apache-2.0 OR MIT
use crate::{
	error::{Error, Result},
	header::{FileIntegrity, Header, RawNode},
};
use std::{
	borrow::Cow,
	collections::{HashMap, HashSet},
	path::{Path, PathBuf},
};

/// An optimized AsarReader that uses flat $O(1)$ hash tables with normalized fast-paths,
/// eliminating the recursive tree traversals and BTreeMap logarithmic overhead of asar-rs.
#[derive(Debug, Clone, PartialEq)]
pub struct AsarReader<'a> {
	header: Header,
	directories: HashMap<PathBuf, Vec<PathBuf>>,
	files: HashMap<PathBuf, AsarFile<'a>>,
	norm_files: HashMap<String, PathBuf>,
	dirs_set: HashSet<String>,
	symlinks: HashMap<PathBuf, PathBuf>,
	asar_path: Option<PathBuf>,
	data_offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AsarFile<'a> {
	data: Cow<'a, [u8]>,
	offset: usize,
	size: usize,
	executable: bool,
	integrity: Option<FileIntegrity>,
}

impl<'a> AsarFile<'a> {
	#[inline]
	pub fn data(&self) -> &[u8] {
		self.data.as_ref()
	}

	#[inline]
	pub const fn offset(&self) -> usize {
		self.offset
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

#[inline]
pub fn normalize_str_path(path: &str) -> String {
	let mut s = String::with_capacity(path.len());
	let trimmed = path.trim_start_matches(|c| c == '/' || c == '\\');
	for b in trimmed.chars() {
		if b == '\\' {
			s.push('/');
		} else {
			s.extend(b.to_lowercase());
		}
	}
	s
}

impl<'a> AsarReader<'a> {
	/// Parse and read an asar archive from a byte buffer.
	pub fn new(data: &'a [u8], asar_path: impl Into<Option<PathBuf>>) -> Result<Self> {
		let (raw, begin_offset) = Header::read_from_slice(data)?;
		Self::from_raw_node(raw, begin_offset, data, asar_path)
	}

	/// Construct AsarReader directly from a parsed RawNode and byte slice.
	pub fn from_raw_node(
		raw: RawNode,
		begin_offset: usize,
		data: &'a [u8],
		asar_path: impl Into<Option<PathBuf>>,
	) -> Result<Self> {
		let asar_path = asar_path.into();
		let mut files = HashMap::new();
		let mut norm_files = HashMap::new();
		let mut directories = HashMap::new();
		let mut dirs_set = HashSet::new();
		let mut symlinks = HashMap::new();

		dirs_set.insert(String::new());

		let mut path_buf = String::with_capacity(256);
		flatten_node(
			raw,
			&mut path_buf,
			&mut files,
			&mut norm_files,
			&mut directories,
			&mut dirs_set,
			&mut symlinks,
			begin_offset,
			data,
			asar_path.as_deref(),
		)?;

		Ok(Self {
			header: Header::new(),
			directories,
			files,
			norm_files,
			dirs_set,
			symlinks,
			asar_path,
			data_offset: begin_offset,
		})
	}

	#[inline]
	pub const fn files(&self) -> &HashMap<PathBuf, AsarFile<'a>> {
		&self.files
	}

	#[inline]
	pub const fn directories(&self) -> &HashMap<PathBuf, Vec<PathBuf>> {
		&self.directories
	}

	#[inline]
	pub const fn symlinks(&self) -> &HashMap<PathBuf, PathBuf> {
		&self.symlinks
	}

	#[inline]
	pub const fn data_offset(&self) -> usize {
		self.data_offset
	}

	/// Instant O(1) check if a directory exists (case and slash insensitive).
	#[inline]
	pub fn has_dir(&self, dir: &str) -> bool {
		let norm = normalize_str_path(dir);
		norm.is_empty() || self.dirs_set.contains(&norm)
	}

	/// Instant O(1) check if a file exists (case and slash insensitive).
	#[inline]
	pub fn has_file(&self, file: &str) -> bool {
		let path = Path::new(file);
		if self.files.contains_key(path) {
			return true;
		}
		let norm = normalize_str_path(file);
		self.norm_files.contains_key(&norm)
	}

	/// Gets information about a file with instant O(1) hash table lookup.
	/// Automatically resolves symlinks, slashes ('\' vs '/'), and case variations without tree walking.
	#[inline]
	pub fn read(&self, path: &Path) -> Option<&AsarFile<'a>> {
		// 1. Direct exact match in O(1) (hits in 99% of requests)
		if let Some(file) = self.files.get(path) {
			return Some(file);
		}

		// 2. Symlink check only if archive contains symlinks
		if !self.symlinks.is_empty() {
			if let Some(link) = self.symlinks.get(path) {
				return self.files.get(link);
			}
		}

		// 3. Normalized fast-path: slash and case insensitive lookup in O(1)
		let path_str = path.to_str()?;
		let norm = normalize_str_path(path_str);
		if let Some(exact_key) = self.norm_files.get(&norm) {
			if !self.symlinks.is_empty() {
				if let Some(link) = self.symlinks.get(exact_key) {
					return self.files.get(link);
				}
			}
			return self.files.get(exact_key);
		}

		None
	}

	/// Read raw byte slice directly in O(1) using a string slice path.
	#[inline]
	pub fn read_bytes(&self, path: &str) -> Option<&[u8]> {
		self.read(Path::new(path)).map(|f| f.data())
	}

	/// Gets the contents of a directory in O(1).
	#[inline]
	pub fn read_dir(&self, path: &Path) -> Option<&[PathBuf]> {
		self.directories.get(path).map(|paths| paths.as_slice())
	}
}

fn flatten_node<'a>(
	node: RawNode,
	path_buf: &mut String,
	files: &mut HashMap<PathBuf, AsarFile<'a>>,
	norm_files: &mut HashMap<String, PathBuf>,
	directories: &mut HashMap<PathBuf, Vec<PathBuf>>,
	dirs_set: &mut HashSet<String>,
	symlinks: &mut HashMap<PathBuf, PathBuf>,
	begin_offset: usize,
	data: &'a [u8],
	asar_path: Option<&Path>,
) -> Result<()> {
	if let Some(link_target) = node.link {
		let path = PathBuf::from(&*path_buf);
		let target = PathBuf::from(link_target);
		symlinks.insert(path, target);
	} else if let Some(dir_children) = node.files {
		let dir_path = PathBuf::from(&*path_buf);
		let norm_dir = normalize_str_path(path_buf);
		dirs_set.insert(norm_dir);

		let prev_len = path_buf.len();
		let mut children_vec = Vec::with_capacity(dir_children.len());

		for (child_name, child_node) in dir_children {
			if !path_buf.is_empty() {
				path_buf.push('/');
			}
			path_buf.push_str(&child_name);

			let child_path = PathBuf::from(&*path_buf);
			children_vec.push(child_path);

			flatten_node(
				child_node,
				path_buf,
				files,
				norm_files,
				directories,
				dirs_set,
				symlinks,
				begin_offset,
				data,
				asar_path,
			)?;

			path_buf.truncate(prev_len);
		}

		directories.insert(dir_path, children_vec);
	} else {
		let size = node.size.unwrap_or(0);
		let unpacked = node.unpacked.unwrap_or(false);
		let executable = node.executable.unwrap_or(false);
		let offset = node.offset.map(|o| o.0).unwrap_or(0);

		let path = PathBuf::from(&*path_buf);
		let norm_key = normalize_str_path(path_buf);

		let data_slice: Cow<'a, [u8]> = if unpacked {
			match asar_path {
				Some(asar_root) => {
					let full_unpacked = asar_root.with_extension("asar.unpacked").join(&path);
					match std::fs::read(&full_unpacked) {
						Ok(b) => Cow::Owned(b),
						Err(err) => return Err(Error::UnpackedIoError { path, err }),
					}
				}
				None => Cow::Borrowed(&[] as &[u8]),
			}
		} else {
			let start = begin_offset + offset;
			let end = start + size;
			if data.len() < end {
				return Err(Error::Truncated);
			}
			Cow::Borrowed(&data[start..end])
		};

		norm_files.insert(norm_key, path.clone());
		files.insert(
			path,
			AsarFile {
				data: data_slice,
				offset,
				size,
				executable,
				integrity: node.integrity,
			},
		);
	}

	Ok(())
}

#[cfg(test)]
pub mod test {
	use super::*;
	use crate::header::TEST_ASAR;
	use include_dir::{include_dir, Dir};

	static ASAR_CONTENTS: Dir = include_dir!("$CARGO_MANIFEST_DIR/data/contents");

	#[test]
	fn test_reading_contents() {
		let reader = AsarReader::new(TEST_ASAR, None).expect("failed to read asar");
		for (path, file) in reader.files() {
			let real_file = ASAR_CONTENTS
				.get_file(path)
				.unwrap_or_else(|| panic!("test.asar contains invalid file {}", path.display()));
			let real_contents = real_file.contents();
			let asar_contents = file.data();
			assert_eq!(real_contents, asar_contents);
		}
	}

	#[test]
	fn test_o1_lookup_and_case_slash_insensitivity() {
		let reader = AsarReader::new(TEST_ASAR, None).expect("failed to read asar");
		assert!(reader.has_file("file 1.txt"));
		assert!(reader.has_file("FILE 1.TXT"));
		assert!(reader.has_file("folder 1/folder 2/file 4.txt"));
		assert!(reader.has_file("folder 1\\folder 2\\file 4.txt"));
		assert!(reader.has_dir("folder 1"));
		assert!(reader.has_dir("folder 1/folder 2"));
		assert!(reader.has_dir("FOLDER 1\\FOLDER 2"));

		let file = reader.read(Path::new("folder 1\\folder 2\\file 4.txt")).unwrap();
		assert!(!file.data().is_empty());
	}
}
