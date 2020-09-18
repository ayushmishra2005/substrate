// This file is part of Substrate.

// Copyright (C) 2020 Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Merkle Mountain Range utilities.

/// MMR nodes utilities.
pub struct NodesUtils {
	no_of_leaves: u64,
}

impl NodesUtils {
	/// Create new instance of MMR nodes utilities.
	pub fn new(no_of_leaves: u64) -> Self {
		Self { no_of_leaves }
	}

	/// Return number of peaks in the MMR.
	pub fn number_of_peaks(&self) -> u64 {
		self.number_of_leaves().count_ones() as u64
	}

	/// Return the number of leaves in the MMR.
	pub fn number_of_leaves(&self) -> u64 {
		self.no_of_leaves
	}

	/// The total size of the MMR (number of nodes).
	pub fn size(&self) -> u64 {
		2 * self.no_of_leaves - self.number_of_peaks()
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn should_calculate_number_of_leaves_correctly() {
		assert_eq!(
			vec![0, 1, 2, 3, 4, 9, 15, 21]
				.into_iter()
				.map(|n| NodesUtils::new(n).number_of_leaves())
				.collect::<Vec<_>>(),
			vec![0, 1, 2, 3, 4, 9, 15, 21]
		);
	}

	#[test]
	fn should_calculate_number_of_peaks_correctly() {
		assert_eq!(
			vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 21]
				.into_iter()
				.map(|n| NodesUtils::new(n).number_of_peaks())
				.collect::<Vec<_>>(),
			vec![0, 1, 1, 2, 1, 2, 2, 3, 1, 2, 2, 3, 2, 3, 3, 4, 3]
		);
	}

	#[test]
	fn should_calculate_the_size_correctly() {
		let _ = env_logger::try_init();

		let leaves = vec![0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 21];
		let sizes = vec![0, 1, 3, 4, 7, 8, 10, 11, 15, 16, 18, 19, 22, 23, 25, 26, 39];
		assert_eq!(
			leaves
				.clone()
				.into_iter()
				.map(|n| NodesUtils::new(n).size())
				.collect::<Vec<_>>(),
			sizes.clone()
		);

		// size cross-check
		let mut actual_sizes = vec![];
		for s in &leaves[1..] {
			crate::tests::new_test_ext().execute_with(|| {
				let mut mmr = crate::mmr::MMR::<crate::mmr::storage::RuntimeStorage, crate::tests::Test, _>::new(0);
				for i in 0..*s {
					mmr.push(i);
				}
				actual_sizes.push(mmr.size());
			})
		}
		assert_eq!(
			sizes[1..],
			actual_sizes[..],
		);
	}
}