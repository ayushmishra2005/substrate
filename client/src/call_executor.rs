// Copyright 2017-2019 Parity Technologies (UK) Ltd.
// This file is part of Substrate.

// Substrate is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Substrate is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Substrate.  If not, see <http://www.gnu.org/licenses/>.

use std::{panic::UnwindSafe, result, cell::RefCell};
use codec::{Encode, Decode};
use sp_runtime::{
	generic::BlockId, traits::Block as BlockT, traits::NumberFor,
};
use state_machine::{
	self, OverlayedChanges, Ext, ExecutionManager, StateMachine, ExecutionStrategy,
	backend::Backend as _, ChangesTrieTransaction, StorageProof,
};
use executor::{RuntimeVersion, RuntimeInfo, NativeVersion};
use externalities::Extensions;
use hash_db::Hasher;
use primitives::{
	H256, Blake2Hasher, NativeOrEncoded, NeverNativeValue,
	traits::CodeExecutor,
};
use sp_api::{ProofRecorder, InitializeBlock};
use client_api::{backend, call_executor::CallExecutor};

/// Call executor that executes methods locally, querying all required
/// data from local backend.
pub struct LocalCallExecutor<E, B> {
	executor: E,
	_marker: std::marker::PhantomData<B>,
}

impl<E, B> LocalCallExecutor<E, B> {
	/// Creates new instance of local call executor.
	pub fn new(executor: E) -> Self {
		Self {
			executor,
			_marker: Default::default()
		}
	}
}

impl<E, B> Clone for LocalCallExecutor<E, B>
where
	E: Clone,
{
	fn clone(&self) -> Self {
		LocalCallExecutor {
			executor: self.executor.clone(),
			_marker: self._marker,
		}
	}
}

impl<B, E, Block> CallExecutor<Block, Blake2Hasher, B> for LocalCallExecutor<E, B>
where
	B: backend::Backend<Block, Blake2Hasher>,
	E: CodeExecutor + RuntimeInfo,
	Block: BlockT<Hash=H256>,
{
	type Error = E::Error;

	fn call(
		&self,
		backend: &B,
		id: &BlockId<Block>,
		method: &str,
		call_data: &[u8],
		strategy: ExecutionStrategy,
		extensions: Option<Extensions>,
	) -> sp_blockchain::Result<Vec<u8>> {
		let mut changes = OverlayedChanges::default();
		let state = backend.state_at(*id)?;
		let return_data = StateMachine::new(
			&state,
			backend.changes_trie_storage(),
			&mut changes,
			&self.executor,
			method,
			call_data,
			extensions.unwrap_or_default(),
		).execute_using_consensus_failure_handler::<_, NeverNativeValue, fn() -> _>(
			strategy.get_manager(),
			false,
			None,
		)
		.map(|(result, _, _)| result)?;
		let _lock = backend.get_import_lock().read();
		backend.destroy_state(state)?;
		Ok(return_data.into_encoded())
	}

	fn contextual_call<
		'a,
		IB: Fn() -> sp_blockchain::Result<()>,
		EM: Fn(
			Result<NativeOrEncoded<R>, Self::Error>,
			Result<NativeOrEncoded<R>, Self::Error>
		) -> Result<NativeOrEncoded<R>, Self::Error>,
		R: Encode + Decode + PartialEq,
		NC: FnOnce() -> result::Result<R, String> + UnwindSafe,
	>(
		&self,
		backend: &B,
		initialize_block_fn: IB,
		at: &BlockId<Block>,
		method: &str,
		call_data: &[u8],
		changes: &RefCell<OverlayedChanges>,
		initialize_block: InitializeBlock<'a, Block>,
		execution_manager: ExecutionManager<EM>,
		native_call: Option<NC>,
		recorder: &Option<ProofRecorder<Block>>,
		extensions: Option<Extensions>,
	) -> Result<NativeOrEncoded<R>, sp_blockchain::Error> where ExecutionManager<EM>: Clone {
		match initialize_block {
			InitializeBlock::Do(ref init_block)
				if init_block.borrow().as_ref().map(|id| id != at).unwrap_or(true) => {
				initialize_block_fn()?;
			},
			// We don't need to initialize the runtime at a block.
			_ => {},
		}

		let mut state = backend.state_at(*at)?;

		let result = match recorder {
			Some(recorder) => {
				let trie_state = state.as_trie_backend()
					.ok_or_else(||
						Box::new(state_machine::ExecutionError::UnableToGenerateProof)
							as Box<dyn state_machine::Error>
					)?;

				let b = state_machine::ProvingBackend::new_with_recorder(
					trie_state,
					recorder.clone()
				);

				StateMachine::new(
					&b,
					backend.changes_trie_storage(),
					&mut *changes.borrow_mut(),
					&self.executor,
					method,
					call_data,
					extensions.unwrap_or_default(),
				)
				.execute_using_consensus_failure_handler(
					execution_manager,
					false,
					native_call,
				)
				.map(|(result, _, _)| result)
				.map_err(Into::into)
			}
			None => StateMachine::new(
				&state,
				backend.changes_trie_storage(),
				&mut *changes.borrow_mut(),
				&self.executor,
				method,
				call_data,
				extensions.unwrap_or_default(),
			)
			.execute_using_consensus_failure_handler(
				execution_manager,
				false,
				native_call,
			)
			.map(|(result, _, _)| result)
		}?;
		let _lock = backend.get_import_lock().read();
		backend.destroy_state(state)?;
		Ok(result)
	}

	fn runtime_version(
		&self,
		backend: &B,
		id: &BlockId<Block>
	) -> sp_blockchain::Result<RuntimeVersion> {
		let mut overlay = OverlayedChanges::default();
		let state = backend.state_at(*id)?;

		let mut ext = Ext::new(
			&mut overlay,
			&state,
			backend.changes_trie_storage(),
			None,
		);
		let version = self.executor.runtime_version(&mut ext);
		{
			let _lock = backend.get_import_lock().read();
			backend.destroy_state(state)?;
		}
		version.map_err(|e| sp_blockchain::Error::VersionInvalid(format!("{:?}", e)).into())
	}

	fn call_at_state<
		S: state_machine::Backend<Blake2Hasher>,
		F: FnOnce(
			Result<NativeOrEncoded<R>, Self::Error>,
			Result<NativeOrEncoded<R>, Self::Error>,
		) -> Result<NativeOrEncoded<R>, Self::Error>,
		R: Encode + Decode + PartialEq,
		NC: FnOnce() -> result::Result<R, String> + UnwindSafe,
	>(&self,
		backend: &B,
		state: &S,
		changes: &mut OverlayedChanges,
		method: &str,
		call_data: &[u8],
		manager: ExecutionManager<F>,
		native_call: Option<NC>,
		extensions: Option<Extensions>,
	) -> sp_blockchain::Result<(
		NativeOrEncoded<R>,
		(S::Transaction, <Blake2Hasher as Hasher>::Out),
		Option<ChangesTrieTransaction<Blake2Hasher, NumberFor<Block>>>,
	)> {
		StateMachine::new(
			state,
			backend.changes_trie_storage(),
			changes,
			&self.executor,
			method,
			call_data,
			extensions.unwrap_or_default(),
		).execute_using_consensus_failure_handler(
			manager,
			true,
			native_call,
		)
		.map(|(result, storage_tx, changes_tx)| (
			result,
			storage_tx.expect("storage_tx is always computed when compute_tx is true; qed"),
			changes_tx,
		))
		.map_err(Into::into)
	}

	fn prove_at_trie_state<S: state_machine::TrieBackendStorage<Blake2Hasher>>(
		&self,
		trie_state: &state_machine::TrieBackend<S, Blake2Hasher>,
		overlay: &mut OverlayedChanges,
		method: &str,
		call_data: &[u8]
	) -> Result<(Vec<u8>, StorageProof), sp_blockchain::Error> {
		state_machine::prove_execution_on_trie_backend(
			trie_state,
			overlay,
			&self.executor,
			method,
			call_data,
		)
		.map_err(Into::into)
	}

	fn native_runtime_version(&self) -> Option<&NativeVersion> {
		Some(self.executor.native_version())
	}
}

impl<B, E, Block> runtime_version::GetRuntimeVersion<Block> for LocalCallExecutor<E, B>
	where
		B: backend::Backend<Block, Blake2Hasher>,
		E: CodeExecutor + RuntimeInfo,
		Block: BlockT<Hash=H256>,
{
	fn native_version(&self) -> &runtime_version::NativeVersion {
		self.executor.native_version()
	}

	fn runtime_version(
		&self,
		at: &BlockId<Block>,
	) -> Result<runtime_version::RuntimeVersion, String> {
		CallExecutor::runtime_version(self, todo!(), at).map_err(|e| format!("{:?}", e))
	}
}
