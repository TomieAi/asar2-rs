// SPDX-License-Identifier: Apache-2.0 OR MIT
use crate::{
	error::{Error, Result},
	header::{File, FileIntegrity, FileLocation, HashAlgorithm, Header},
	reader::AsarReader,
};
use byteorder::{LittleEndian, WriteBytesExt};
use std::{
	collections::{BTreeMap, BTreeSet, VecDeque},
	io::Write,
	path::{Component, Path, PathBuf},
};

const BLOCK_SIZE: usize = 4 * 1024 * 1024; // 4 MiB

pub struct AsarWriter {
	directories: BTreeSet<PathBuf>,
	files: BTreeMap<PathBuf, File>,
	symlinks: BTreeMap<PathBuf, PathBuf>,
	buffer: Vec<u8>,
	offset: usize,
	hasher: HashAlgorithm,
}

impl AsarWriter {
	pub fn new() -> Self {
		Self::default()
	}

	pub fn new_with_algorithm(hasher: HashAlgorithm) -> Self {
		Self {
			directories: BTreeSet::new(),
			files: BTreeMap::new(),
			symlinks: BTreeMap::new(),
			buffer: Vec::new(),
			offset: 0,
			hasher,
		}
	}

	pub fn add_from_reader(&mut self, reader: &AsarReader) -> Result<()> {
		for (path, file) in reader.files() {
			self.write_file(path, file.data(), false)?;
		}
		Ok(())
	}

	pub fn write_file(
		&mut self,
		path: impl AsRef<Path>,
		bytes: impl AsRef<[u8]>,
		executable: bool,
	) -> Result<()> {
		self.write_file_impl(path.as_ref(), bytes.as_ref(), executable)
	}

	pub fn write_dir(&mut self, path: impl AsRef<Path>) -> Result<()> {
		self.directories.insert(path.as_ref().to_path_buf());
		Ok(())
	}

	pub fn write_symlink(&mut self, path: impl AsRef<Path>, link: impl AsRef<Path>) -> Result<()> {
		self.symlinks
			.insert(path.as_ref().to_path_buf(), link.as_ref().to_path_buf());
		Ok(())
	}

	fn write_file_impl(&mut self, path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
		if self.files.contains_key(path) {
			return Err(Error::FileAlreadyWritten(path.to_path_buf()));
		}
		let file = File::new(
			FileLocation::Offset {
				offset: self.offset,
			},
			bytes.len(),
			executable,
			Some(FileIntegrity::new(
				self.hasher,
				self.hasher.hash(bytes),
				BLOCK_SIZE,
				self.hasher.hash_blocks(BLOCK_SIZE, bytes),
			)),
		);
		self.buffer.extend_from_slice(bytes);
		self.offset += bytes.len();
		self.files.insert(path.to_path_buf(), file);
		Ok(())
	}

	pub fn finalize<FinalWriter>(self, mut final_writer: FinalWriter) -> Result<usize>
	where
		FinalWriter: Write,
	{
		let mut header = Header::new();
		for dir in self.directories {
			let comps = path_to_reverse_components(&dir)?;
			recursive_add_dir_to_header(comps, &mut header);
		}
		for (path, file) in self.files {
			let comps = path_to_reverse_components(&path)?;
			recursive_add_to_header(comps, Header::File(file), &mut header);
		}
		for (path, link) in self.symlinks {
			let comps = path_to_reverse_components(&path)?;
			recursive_add_to_header(comps, Header::Link { link }, &mut header);
		}
		let mut written = 0;
		let mut json = serde_json::to_string(&header)?.into_bytes();

		let json_size = json.len() as u32;
		let aligned_json_size = json_size + (4 - (json_size % 4)) % 4;
		json.resize(aligned_json_size as usize, 0);

		final_writer.write_u32::<LittleEndian>(4)?;
		written += std::mem::size_of::<u32>();
		final_writer.write_u32::<LittleEndian>(aligned_json_size + 8)?;
		written += std::mem::size_of::<u32>();
		final_writer.write_u32::<LittleEndian>(aligned_json_size + 4)?;
		written += std::mem::size_of::<u32>();
		final_writer.write_u32::<LittleEndian>(json_size)?;
		written += std::mem::size_of::<u32>();
		final_writer.write_all(&json)?;
		written += json.len();
		final_writer.write_all(&self.buffer)?;
		written += self.buffer.len();
		final_writer.flush()?;
		Ok(written)
	}
}

impl Default for AsarWriter {
	fn default() -> Self {
		Self {
			directories: BTreeSet::new(),
			files: BTreeMap::new(),
			symlinks: BTreeMap::new(),
			offset: 0,
			buffer: Vec::new(),
			hasher: HashAlgorithm::Sha256,
		}
	}
}

fn recursive_add_dir_to_header(mut path: VecDeque<String>, header: &mut Header) {
	let header_map = match header {
		Header::Directory { files } => files,
		_ => return,
	};
	if let Some(comp) = path.pop_front() {
		let entry = header_map.entry(comp).or_insert_with(Header::new);
		recursive_add_dir_to_header(path, entry);
	}
}

fn path_to_reverse_components(path: &Path) -> Result<VecDeque<String>> {
	Ok(path
		.components()
		.filter_map(|c| match c {
			Component::Prefix(_) | Component::RootDir => None,
			Component::ParentDir | Component::CurDir => None,
			Component::Normal(p) => Some(
				p.to_str()
					.map(str::to_string)
					.unwrap_or_else(|| p.to_string_lossy().into_owned()),
			),
		})
		.collect())
}

fn recursive_add_to_header(mut path: VecDeque<String>, file_or_symlink: Header, header: &mut Header) {
	let header_map = match header {
		Header::Directory { files } => files,
		_ => return,
	};
	match path.pop_front() {
		Some(name) if path.is_empty() => {
			header_map.insert(name, file_or_symlink);
		}
		Some(name) => {
			let new_header = header_map.entry(name).or_insert_with(Header::new);
			recursive_add_to_header(path, file_or_symlink, new_header);
		}
		None => {}
	};
}

#[cfg(test)]
mod test {
	use super::*;
	use crate::header::TEST_ASAR;
	use std::io::Cursor;

	#[test]
	pub fn round_trip() {
		let reader_a = AsarReader::new(TEST_ASAR, None).expect("failed to read asar");
		let mut writer = AsarWriter::new();
		writer.add_from_reader(&reader_a).expect("failed to add asar");
		let mut out = Cursor::new(Vec::<u8>::new());
		writer.finalize(&mut out).expect("failed to finalize asar");
		let out = out.into_inner();
		let reader_b = AsarReader::new(&out, None).expect("failed to read new asar");
		assert_eq!(reader_a.files().len(), reader_b.files().len());
		for (k, v) in reader_a.files() {
			let v2 = reader_b.files().get(k).unwrap_or_else(|| panic!("missing {:?}", k));
			assert_eq!(v.data(), v2.data());
		}
	}
}

