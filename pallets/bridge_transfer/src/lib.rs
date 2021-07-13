// Ensure we're `no_std` when compiling for Wasm.
#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use frame_support::traits::{Currency, EnsureOrigin, ExistenceRequirement::AllowDeath};
use frame_support::{
	decl_error, decl_event, decl_module, decl_storage, dispatch::DispatchResult, ensure, fail,
};
use frame_system::{self as system, ensure_root, ensure_signed};
use pallet_bridge as bridge;
use sp_arithmetic::traits::SaturatedConversion;
use sp_core::U256;
use sp_std::convert::TryFrom;
use sp_std::prelude::*;

use phala_pallets::pallet_mq;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

type ResourceId = bridge::ResourceId;

type BalanceOf<T> =
	<<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

phala_types::messaging::bind_topic!(LotteryEvent, b"phala/lottery/event");
#[derive(Decode, Encode, Debug, PartialEq, Eq, Clone)]
pub enum LotteryEvent {
	/// Receive command: Newround. [roundId, totalCount, winnerCount]
	NewRound(u32, u32, u32),
	/// Receive commnad: Openbox. [roundId, tokenId, btcAddress]
	OpenBox(u32, u32, Vec<u8>),
}

pub trait Config: system::Config + bridge::Config + pallet_mq::Config {
	type Event: From<Event<Self>> + Into<<Self as frame_system::Config>::Event>;

	/// Specifies the origin check provided by the bridge for calls that can only be called by the bridge pallet
	type BridgeOrigin: EnsureOrigin<Self::Origin, Success = Self::AccountId>;

	/// The currency mechanism.
	type Currency: Currency<Self::AccountId>;
}

decl_storage! {
	trait Store for Module<T: Config> as BridgeTransfer {
		BridgeTokenId get(fn bridge_tokenid): ResourceId;
		BridgeLotteryId get(fn bridge_lotteryid): ResourceId;
		BridgeFee get(fn bridge_fee): map hasher(opaque_blake2_256) bridge::ChainId => (BalanceOf<T>, u32);
	}

	add_extra_genesis {
		config(bridge_tokenid): ResourceId;
		config(bridge_lotteryid): ResourceId;
		build(|config: &GenesisConfig| {
			BridgeTokenId::put(config.bridge_tokenid);
			BridgeLotteryId::put(config.bridge_lotteryid);
		});
	}
}

decl_event! {
	pub enum Event<T>
	where
		Balance = BalanceOf<T>,
	{
		/// [chainId, min_fee, fee_scale]
		FeeUpdated(bridge::ChainId, Balance, u32),
	}
}

decl_error! {
	pub enum Error for Module<T: Config>{
		InvalidTransfer,
		InvalidCommand,
		InvalidPayload,
		InvalidFeeOption,
		FeeOptionsMissiing,
	}
}

decl_module! {
	pub struct Module<T: Config> for enum Call where origin: T::Origin {
		type Error = Error<T>;
		//
		// Initiation calls. These start a bridge transfer.
		//

		fn deposit_event() = default;

		/// Change extra bridge transfer fee that user should pay
		#[weight = 195_000_000]
		pub fn sudo_change_fee(origin, min_fee: BalanceOf<T>, fee_scale: u32, dest_id: bridge::ChainId) -> DispatchResult {
			ensure_root(origin)?;
			ensure!(fee_scale <= 1000u32, Error::<T>::InvalidFeeOption);
			BridgeFee::<T>::insert(dest_id, (min_fee, fee_scale));
			Self::deposit_event(RawEvent::FeeUpdated(dest_id, min_fee, fee_scale));
			Ok(())
		}

		/// Transfers an arbitrary signed bitcoin tx to a (whitelisted) destination chain.
		#[weight = 195_000_000]
		pub fn force_lottery_output(origin, payload: Vec<u8>, dest_id: bridge::ChainId) -> DispatchResult {
			ensure_root(origin)?;
			let lottery = Lottery::decode(&mut &payload[..])
				.or(Err(Error::<T>::InvalidPayload))?;
			Self::lottery_output(&lottery, dest_id)
		}

		/// Transfers some amount of the native token to some recipient on a (whitelisted) destination chain.
		#[weight = 195_000_000]
		pub fn transfer_native(origin, amount: BalanceOf<T>, recipient: Vec<u8>, dest_id: bridge::ChainId) -> DispatchResult {
			let source = ensure_signed(origin)?;
			ensure!(<bridge::Module<T>>::chain_whitelisted(dest_id), Error::<T>::InvalidTransfer);
			let bridge_id = <bridge::Module<T>>::account_id();
			ensure!(BridgeFee::<T>::contains_key(&dest_id), Error::<T>::FeeOptionsMissiing);
			let (min_fee, fee_scale) = Self::bridge_fee(dest_id);
			let fee_estimated = amount * fee_scale.into() / 1000u32.into();
			let fee = if fee_estimated > min_fee {
				fee_estimated
			} else {
				min_fee
			};
			T::Currency::transfer(&source, &bridge_id, (amount + fee).into(), AllowDeath)?;

			let resource_id = Self::bridge_tokenid();

			<bridge::Module<T>>::transfer_fungible(dest_id, resource_id, recipient, U256::from(amount.saturated_into::<u128>()))
		}

		//
		// Executable calls. These can be triggered by a bridge transfer initiated on another chain
		//

		/// Executes a simple currency transfer using the bridge account as the source
		#[weight = 195_000_000]
		pub fn transfer(origin, to: T::AccountId, amount: BalanceOf<T>, _rid: ResourceId) -> DispatchResult {
			let source = T::BridgeOrigin::ensure_origin(origin)?;
			<T as Config>::Currency::transfer(&source, &to, amount.into(), AllowDeath)?;
			Ok(())
		}

		/// This can be called by the bridge to demonstrate an arbitrary call from a proposal.
		#[weight = 195_000_000]
		pub fn lottery_handler(origin, metadata: Vec<u8>, _rid: ResourceId) -> DispatchResult {
			T::BridgeOrigin::ensure_origin(origin)?;

			let op = u8::from_be_bytes(<[u8; 1]>::try_from(&metadata[..1]).map_err(|_| Error::<T>::InvalidCommand)?);
			if op == 0 {
				ensure!(
					metadata.len() == 13,
					Error::<T>::InvalidCommand
				);

				Self::push_message(LotteryEvent::NewRound(
					u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[1..5]).map_err(|_| Error::<T>::InvalidCommand)?),	// roundId
					u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[5..9]).map_err(|_| Error::<T>::InvalidCommand)?),	// totalCount
					u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[9..]).map_err(|_| Error::<T>::InvalidCommand)?)	// winnerCount
				));
			} else if op == 1 {
				ensure!(
					metadata.len() > 13,
					Error::<T>::InvalidCommand
				);

				let address_len: usize = u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[9..13]).map_err(|_| Error::<T>::InvalidCommand)?).saturated_into();
				ensure!(
					metadata.len() == (13 + address_len),
					Error::<T>::InvalidCommand
				);

				Self::push_message(LotteryEvent::OpenBox(
					u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[1..5]).map_err(|_| Error::<T>::InvalidCommand)?),	// roundId
					u32::from_be_bytes(<[u8; 4]>::try_from(&metadata[5..9]).map_err(|_| Error::<T>::InvalidCommand)?),	// tokenId
					metadata[13..].to_vec()						// btcAddress
				));
			} else {
				fail!(Error::<T>::InvalidCommand);
			}

			Ok(())
		}

		#[weight = 0]
		fn force_lottery_new_round(origin, round_id: u32, total_count: u32, winner_count: u32) -> DispatchResult {
			ensure_root(origin)?;
			Self::push_message(LotteryEvent::NewRound(round_id, total_count, winner_count));
			Ok(())
		}

		#[weight = 0]
		fn force_lottery_open_box(origin, round_id: u32, token_id: u32, btc_address: Vec<u8>) -> DispatchResult {
			ensure_root(origin)?;
			Self::push_message(LotteryEvent::OpenBox(round_id, token_id, btc_address));
			Ok(())
		}
	}
}

use phala_types::messaging::{BindTopic, Lottery, DecodedMessage, MessageOrigin};

impl<T: Config> Module<T> {
	pub fn lottery_output(payload: &Lottery, dest_id: bridge::ChainId) -> DispatchResult {
		ensure!(
			<bridge::Module<T>>::chain_whitelisted(dest_id),
			Error::<T>::InvalidTransfer
		);
		let resource_id = Self::bridge_lotteryid();
		let metadata: Vec<u8> = payload.encode();
		<bridge::Module<T>>::transfer_generic(dest_id, resource_id, metadata)
	}

	fn push_message(payload: impl Encode + BindTopic) {
		pallet_mq::Pallet::<T>::push_bound_message(Self::message_origin(), payload);
	}

	pub fn message_origin() -> MessageOrigin {
		<Self as pallet_mq::MessageOriginInfo>::message_origin()
	}
}

impl<T: Config> pallet_mq::MessageOriginInfo for Module<T> {
	type Config = T;
}

impl<T: Config> Module<T> {
	pub fn on_message_received(message: DecodedMessage<Lottery>) -> DispatchResult {
		// TODO.kevin: check the sender?
		// Dest chain 0 is EVM chain, and 1 is ourself
		Self::lottery_output(&message.payload, 0)
	}
}
