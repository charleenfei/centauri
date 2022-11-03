#![allow(unreachable_patterns)]

use async_trait::async_trait;
use derive_more::From;
use futures::Stream;
#[cfg(any(test, feature = "testing"))]
use ibc::applications::transfer::msgs::transfer::MsgTransfer;
use ibc::{
	applications::transfer::PrefixedCoin,
	core::{
		ics02_client::client_state::ClientType,
		ics23_commitment::commitment::CommitmentPrefix,
		ics24_host::identifier::{ChannelId, ClientId, ConnectionId, PortId},
	},
	events::IbcEvent,
	signer::Signer,
	timestamp::Timestamp,
	Height,
};
use ibc_proto::{
	google::protobuf::Any,
	ibc::core::{
		channel::v1::{
			QueryChannelResponse, QueryChannelsResponse, QueryNextSequenceReceiveResponse,
			QueryPacketAcknowledgementResponse, QueryPacketCommitmentResponse,
			QueryPacketReceiptResponse,
		},
		client::v1::{QueryClientStateResponse, QueryConsensusStateResponse},
		connection::v1::{IdentifiedConnection, QueryConnectionResponse},
	},
};
#[cfg(any(test, feature = "testing"))]
use pallet_ibc::Timeout;
use serde::Deserialize;
use thiserror::Error;

use pallet_ibc::light_clients::{AnyClientState, AnyConsensusState};
use parachain::{config, ParachainClient};
use primitives::{Chain, IbcProvider, KeyProvider, UpdateType};
use sp_core::H256;
use sp_runtime::generic::Era;
use std::{pin::Pin, time::Duration};
use subxt::{
	tx::{ExtrinsicParams, PolkadotExtrinsicParams, PolkadotExtrinsicParamsBuilder},
	Error, OnlineClient,
};

// TODO: expose extrinsic param builder
#[derive(Debug, Clone)]
pub enum DefaultConfig {}

#[async_trait]
impl config::Config for DefaultConfig {
	async fn custom_extrinsic_params(
		client: &OnlineClient<Self>,
	) -> Result<
		<Self::ExtrinsicParams as ExtrinsicParams<Self::Index, Self::Hash>>::OtherParams,
		Error,
	> {
		let params =
			PolkadotExtrinsicParamsBuilder::new().era(Era::Immortal, client.genesis_hash());
		Ok(params.into())
	}
}

impl subxt::Config for DefaultConfig {
	type Index = u32;
	type BlockNumber = u32;
	type Hash = sp_core::H256;
	type Hashing = sp_runtime::traits::BlakeTwo256;
	type AccountId = sp_runtime::AccountId32;
	type Address = sp_runtime::MultiAddress<Self::AccountId, u32>;
	type Header = sp_runtime::generic::Header<Self::BlockNumber, sp_runtime::traits::BlakeTwo256>;
	type Signature = sp_runtime::MultiSignature;
	type Extrinsic = sp_runtime::OpaqueExtrinsic;
	type ExtrinsicParams = PolkadotExtrinsicParams<Self>;
}

#[derive(Deserialize)]
pub struct Config {
	pub chain_a: AnyConfig,
	pub chain_b: AnyConfig,
	pub core: CoreConfig,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AnyConfig {
	Parachain(parachain::ParachainClientConfig),
}

#[derive(Deserialize)]
pub struct CoreConfig {
	pub prometheus_endpoint: Option<String>,
}

#[derive(Clone)]
pub enum AnyChain {
	Parachain(ParachainClient<DefaultConfig>),
}

#[derive(From)]
pub enum AnyFinalityEvent {
	Parachain(parachain::finality_protocol::FinalityEvent),
}

#[derive(Error, Debug)]
pub enum AnyError {
	#[error("{0}")]
	Parachain(#[from] parachain::error::Error),
	#[error("{0}")]
	Other(String),
}

impl From<String> for AnyError {
	fn from(s: String) -> Self {
		Self::Other(s)
	}
}

#[async_trait]
impl IbcProvider for AnyChain {
	type FinalityEvent = AnyFinalityEvent;
	type Error = AnyError;

	async fn query_latest_ibc_events<T>(
		&mut self,
		finality_event: Self::FinalityEvent,
		counterparty: &T,
	) -> Result<(Any, Vec<IbcEvent>, UpdateType), anyhow::Error>
	where
		T: Chain,
	{
		match self {
			AnyChain::Parachain(chain) => {
				let finality_event = ibc::downcast!(finality_event => AnyFinalityEvent::Parachain)
					.ok_or_else(|| AnyError::Other("Invalid finality event type".to_owned()))?;
				let (client_msg, events, update_type) =
					chain.query_latest_ibc_events(finality_event, counterparty).await?;
				Ok((client_msg, events, update_type))
			},
			_ => unreachable!(),
		}
	}

	async fn ibc_events(&self) -> Pin<Box<dyn Stream<Item = IbcEvent>>> {
		match self {
			Self::Parachain(chain) => chain.ibc_events().await,
			_ => unreachable!(),
		}
	}

	async fn query_client_consensus(
		&self,
		at: Height,
		client_id: ClientId,
		consensus_height: Height,
	) -> Result<QueryConsensusStateResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain
				.query_client_consensus(at, client_id, consensus_height)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_client_state(
		&self,
		at: Height,
		client_id: ClientId,
	) -> Result<QueryClientStateResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) =>
				chain.query_client_state(at, client_id).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_connection_end(
		&self,
		at: Height,
		connection_id: ConnectionId,
	) -> Result<QueryConnectionResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) =>
				chain.query_connection_end(at, connection_id).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_channel_end(
		&self,
		at: Height,
		channel_id: ChannelId,
		port_id: PortId,
	) -> Result<QueryChannelResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) =>
				chain.query_channel_end(at, channel_id, port_id).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_proof(&self, at: Height, keys: Vec<Vec<u8>>) -> Result<Vec<u8>, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain.query_proof(at, keys).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_packet_commitment(
		&self,
		at: Height,
		port_id: &PortId,
		channel_id: &ChannelId,
		seq: u64,
	) -> Result<QueryPacketCommitmentResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain
				.query_packet_commitment(at, port_id, channel_id, seq)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_packet_acknowledgement(
		&self,
		at: Height,
		port_id: &PortId,
		channel_id: &ChannelId,
		seq: u64,
	) -> Result<QueryPacketAcknowledgementResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain
				.query_packet_acknowledgement(at, port_id, channel_id, seq)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_next_sequence_recv(
		&self,
		at: Height,
		port_id: &PortId,
		channel_id: &ChannelId,
	) -> Result<QueryNextSequenceReceiveResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain
				.query_next_sequence_recv(at, port_id, channel_id)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_packet_receipt(
		&self,
		at: Height,
		port_id: &PortId,
		channel_id: &ChannelId,
		seq: u64,
	) -> Result<QueryPacketReceiptResponse, Self::Error> {
		match self {
			AnyChain::Parachain(chain) => chain
				.query_packet_receipt(at, port_id, channel_id, seq)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn latest_height_and_timestamp(&self) -> Result<(Height, Timestamp), Self::Error> {
		match self {
			AnyChain::Parachain(chain) =>
				chain.latest_height_and_timestamp().await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_packet_commitments(
		&self,
		at: Height,
		channel_id: ChannelId,
		port_id: PortId,
	) -> Result<Vec<u64>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_packet_commitments(at, channel_id, port_id)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_packet_acknowledgements(
		&self,
		at: Height,
		channel_id: ChannelId,
		port_id: PortId,
	) -> Result<Vec<u64>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_packet_acknowledgements(at, channel_id, port_id)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_unreceived_packets(
		&self,
		at: Height,
		channel_id: ChannelId,
		port_id: PortId,
		seqs: Vec<u64>,
	) -> Result<Vec<u64>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_unreceived_packets(at, channel_id, port_id, seqs)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_unreceived_acknowledgements(
		&self,
		at: Height,
		channel_id: ChannelId,
		port_id: PortId,
		seqs: Vec<u64>,
	) -> Result<Vec<u64>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_unreceived_acknowledgements(at, channel_id, port_id, seqs)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	fn channel_whitelist(&self) -> Vec<(ChannelId, PortId)> {
		match self {
			Self::Parachain(chain) => chain.channel_whitelist(),
			_ => unreachable!(),
		}
	}

	async fn query_connection_channels(
		&self,
		at: Height,
		connection_id: &ConnectionId,
	) -> Result<QueryChannelsResponse, Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.query_connection_channels(at, connection_id).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_send_packets(
		&self,
		channel_id: ChannelId,
		port_id: PortId,
		seqs: Vec<u64>,
	) -> Result<Vec<ibc_rpc::PacketInfo>, Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.query_send_packets(channel_id, port_id, seqs).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_recv_packets(
		&self,
		channel_id: ChannelId,
		port_id: PortId,
		seqs: Vec<u64>,
	) -> Result<Vec<ibc_rpc::PacketInfo>, Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.query_recv_packets(channel_id, port_id, seqs).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	fn expected_block_time(&self) -> Duration {
		match self {
			Self::Parachain(chain) => chain.expected_block_time(),
			_ => unreachable!(),
		}
	}

	async fn query_client_update_time_and_height(
		&self,
		client_id: ClientId,
		client_height: Height,
	) -> Result<(Height, Timestamp), Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_client_update_time_and_height(client_id, client_height)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_host_consensus_state_proof(
		&self,
		height: Height,
	) -> Result<Option<Vec<u8>>, Self::Error> {
		match self {
			AnyChain::Parachain(chain) =>
				chain.query_host_consensus_state_proof(height).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_ibc_balance(&self) -> Result<Vec<PrefixedCoin>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain.query_ibc_balance().await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	fn connection_prefix(&self) -> CommitmentPrefix {
		match self {
			AnyChain::Parachain(chain) => chain.connection_prefix(),
			_ => unreachable!(),
		}
	}

	fn client_id(&self) -> ClientId {
		match self {
			AnyChain::Parachain(chain) => chain.client_id(),
			_ => unreachable!(),
		}
	}

	fn connection_id(&self) -> ConnectionId {
		match self {
			AnyChain::Parachain(chain) => chain.connection_id(),
			_ => unreachable!(),
		}
	}

	fn client_type(&self) -> ClientType {
		match self {
			AnyChain::Parachain(chain) => chain.client_type(),
			_ => unreachable!(),
		}
	}

	async fn query_timestamp_at(&self, block_number: u64) -> Result<u64, Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.query_timestamp_at(block_number).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_clients(&self) -> Result<Vec<ClientId>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain.query_clients().await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_channels(&self) -> Result<Vec<(ChannelId, PortId)>, Self::Error> {
		match self {
			Self::Parachain(chain) => chain.query_channels().await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_connection_using_client(
		&self,
		height: u32,
		client_id: String,
	) -> Result<Vec<IdentifiedConnection>, Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.query_connection_using_client(height, client_id).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	fn is_update_required(
		&self,
		latest_height: u64,
		latest_client_height_on_counterparty: u64,
	) -> bool {
		match self {
			Self::Parachain(chain) =>
				chain.is_update_required(latest_height, latest_client_height_on_counterparty),
			_ => unreachable!(),
		}
	}
	async fn initialize_client_state(
		&self,
	) -> Result<(AnyClientState, AnyConsensusState), Self::Error> {
		match self {
			Self::Parachain(chain) => chain.initialize_client_state().await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn query_client_id_from_tx_hash(
		&self,
		tx_hash: H256,
		block_hash: Option<H256>,
	) -> Result<ClientId, Self::Error> {
		match self {
			Self::Parachain(chain) => chain
				.query_client_id_from_tx_hash(tx_hash, block_hash)
				.await
				.map_err(Into::into),
			_ => unreachable!(),
		}
	}
}

impl KeyProvider for AnyChain {
	fn account_id(&self) -> Signer {
		match self {
			AnyChain::Parachain(parachain) => parachain.account_id(),
			_ => unreachable!(),
		}
	}
}

#[async_trait]
impl Chain for AnyChain {
	fn name(&self) -> &str {
		match self {
			Self::Parachain(chain) => chain.name(),
			_ => unreachable!(),
		}
	}

	fn block_max_weight(&self) -> u64 {
		match self {
			Self::Parachain(chain) => chain.block_max_weight(),
			_ => unreachable!(),
		}
	}

	async fn estimate_weight(&self, msg: Vec<Any>) -> Result<u64, Self::Error> {
		match self {
			Self::Parachain(chain) => chain.estimate_weight(msg).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn finality_notifications(
		&self,
	) -> Pin<Box<dyn Stream<Item = Self::FinalityEvent> + Send + Sync>> {
		match self {
			Self::Parachain(chain) => {
				use futures::StreamExt;
				Box::pin(chain.finality_notifications().await.map(|x| x.into()))
			},
			_ => unreachable!(),
		}
	}

	async fn submit(
		&self,
		messages: Vec<Any>,
	) -> Result<(sp_core::H256, Option<sp_core::H256>), Self::Error> {
		match self {
			Self::Parachain(chain) => chain.submit(messages).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}
}

#[cfg(any(test, feature = "testing"))]
#[async_trait]
impl primitives::TestProvider for AnyChain {
	async fn send_transfer(&self, params: MsgTransfer<PrefixedCoin>) -> Result<(), Self::Error> {
		match self {
			Self::Parachain(chain) => chain.send_transfer(params).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn send_ping(&self, channel_id: ChannelId, timeout: Timeout) -> Result<(), Self::Error> {
		match self {
			Self::Parachain(chain) =>
				chain.send_ping(channel_id, timeout).await.map_err(Into::into),
			_ => unreachable!(),
		}
	}

	async fn subscribe_blocks(&self) -> Pin<Box<dyn Stream<Item = u64> + Send + Sync>> {
		match self {
			Self::Parachain(chain) => chain.subscribe_blocks().await,
			_ => unreachable!(),
		}
	}

	fn set_channel_whitelist(&mut self, channel_whitelist: Vec<(ChannelId, PortId)>) {
		match self {
			Self::Parachain(chain) => chain.set_channel_whitelist(channel_whitelist),
			_ => unreachable!(),
		}
	}
}

impl AnyConfig {
	pub async fn into_client(self) -> anyhow::Result<AnyChain> {
		Ok(match self {
			AnyConfig::Parachain(config) =>
				AnyChain::Parachain(ParachainClient::new(config).await?),
		})
	}
}