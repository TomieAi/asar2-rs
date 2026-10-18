// SPDX-License-Identifier: Apache-2.0 OR MIT
use crate::header::HashAlgorithm;
use sha2::{Digest, Sha256};

impl HashAlgorithm {
	pub const fn hash_len(&self) -> usize {
		match self {
			HashAlgorithm::Sha256 => 32,
		}
	}

	pub fn hash(&self, data: &[u8]) -> String {
		match self {
			Self::Sha256 => {
				let mut hasher = Sha256::new();
				hasher.update(data);
				let result = hasher.finalize();
				hex_encode(&result)
			}
		}
	}

	pub fn hash_blocks(&self, block_size: usize, data: &[u8]) -> Vec<String> {
		if block_size == 0 {
			return Vec::new();
		}
		let mut blocks = Vec::with_capacity((data.len() + block_size - 1) / block_size);
		for chunk in data.chunks(block_size) {
			blocks.push(self.hash(chunk));
		}
		blocks
	}
}

pub(crate) fn hex_encode(bytes: &[u8]) -> String {
	const HEX_CHARS: &[u8; 16] = b"0123456789abcdef";
	let mut s = String::with_capacity(bytes.len() * 2);
	for &b in bytes {
		s.push(HEX_CHARS[(b >> 4) as usize] as char);
		s.push(HEX_CHARS[(b & 0xf) as usize] as char);
	}
	s
}

