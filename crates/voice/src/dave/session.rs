use std::collections::HashSet;
use std::fmt::Display;
use std::sync::Arc;

use openmls::prelude::hash_ref::ProposalRef;
use openmls::prelude::{
	BasicCredential, Capabilities, Ciphersuite, Credential, CredentialType, CredentialWithKey, Extension,
	Extensions, ExternalSender, GroupId, KeyPackage, Lifetime, MlsGroup, MlsGroupCreateConfig,
	MlsGroupJoinConfig, MlsMessageBodyOut, MlsMessageIn, OpenMlsProvider, PURE_PLAINTEXT_WIRE_FORMAT_POLICY,
	ProcessedMessageContent, Proposal, ProposalOrRefType, ProtocolMessage, ProtocolVersion, Sender,
	StagedWelcome, VLBytes, Welcome,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use parking_lot::Mutex;
use serde::Deserialize;
use serde_json::{Value, json};
use tls_codec::{DeserializeBytes, Serialize};
use tokio_tungstenite::tungstenite::Message;

use super::ratchet::{KEY_BYTES, KeyRatchet};
use super::transform::{DaveStats, DaveTransform, FrameCrypto};
use crate::error::{Result, VoiceError};
use crate::wire::{ExecuteTransition, PrepareEpoch, PrepareTransition, opcode};

pub const PROTOCOL_VERSION: u8 = 1;

pub(super) const SUITE: Ciphersuite = Ciphersuite::MLS_128_DHKEMP256_AES128GCM_SHA256_P256;
const MEDIA_KEY_LABEL: &str = "Discord Secure Frames v0";
const INITIAL_TRANSITION: u16 = 0;
const FIRST_EPOCH: u64 = 1;
const APPEND_PROPOSALS: u8 = 0;
const REVOKE_PROPOSALS: u8 = 1;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outgoing {
	KeyPackage(Vec<u8>),
	CommitWelcome {
		commit: Vec<u8>,
		welcome: Option<Vec<u8>>,
	},
	TransitionReady(u16),
	InvalidCommitWelcome(u16),
}

impl Outgoing {
	pub fn encode(&self) -> Message {
		match self {
			Self::KeyPackage(key_package) => binary(opcode::DAVE_MLS_KEY_PACKAGE, &[key_package]),
			Self::CommitWelcome { commit, welcome } => binary(
				opcode::DAVE_MLS_COMMIT_WELCOME,
				&[commit, welcome.as_deref().unwrap_or_default()],
			),
			Self::TransitionReady(transition_id) => Message::text(
				json!({
					"op": opcode::DAVE_TRANSITION_READY,
					"d": { "transition_id": transition_id },
				})
				.to_string(),
			),
			Self::InvalidCommitWelcome(transition_id) => Message::text(
				json!({
					"op": opcode::DAVE_MLS_INVALID_COMMIT_WELCOME,
					"d": { "transition_id": transition_id },
				})
				.to_string(),
			),
		}
	}
}

fn binary(op: u8, parts: &[&[u8]]) -> Message {
	let mut bytes = vec![op];
	for part in parts {
		bytes.extend_from_slice(part);
	}
	Message::binary(bytes)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaveEvent {
	Epoch {
		epoch: u64,
		version: u8,
		members: Vec<u64>,
	},
	Failure(String),
}

struct Transition {
	id: u16,
	version: u8,
	own: Option<KeyRatchet>,
	epoch: u64,
	members: Vec<u64>,
}

struct EpochKeys {
	own: KeyRatchet,
	senders: Vec<(u64, KeyRatchet)>,
	epoch: u64,
	members: Vec<u64>,
}

pub struct DaveSession {
	user_id: u64,
	group_id: GroupId,
	provider: OpenMlsRustCrypto,
	signer: SignatureKeyPair,
	credential: CredentialWithKey,
	external_sender: Option<ExternalSender>,
	group: Option<MlsGroup>,
	established: bool,
	announced_version: u8,
	pending: Option<Transition>,
	recognized: HashSet<u64>,
	crypto: Arc<Mutex<FrameCrypto>>,
	stats: Arc<DaveStats>,
	events: Vec<DaveEvent>,
}

impl DaveSession {
	pub fn new(user_id: u64, channel_id: u64) -> Result<Self> {
		let signer = SignatureKeyPair::new(SUITE.signature_algorithm()).map_err(failure("signing key"))?;
		let credential = CredentialWithKey {
			credential: BasicCredential::new(user_id.to_be_bytes().to_vec()).into(),
			signature_key: signer.public().into(),
		};
		let stats = Arc::new(DaveStats::default());
		Ok(Self {
			user_id,
			group_id: GroupId::from_slice(&channel_id.to_be_bytes()),
			provider: OpenMlsRustCrypto::default(),
			signer,
			credential,
			external_sender: None,
			group: None,
			established: false,
			announced_version: 0,
			pending: None,
			recognized: HashSet::from([user_id]),
			crypto: Arc::new(Mutex::new(FrameCrypto::new(stats.clone()))),
			stats,
			events: Vec::new(),
		})
	}

	pub fn transform(&self, resolve: impl Fn(u32) -> Option<u64> + Send + 'static) -> DaveTransform {
		DaveTransform::new(self.crypto.clone(), resolve)
	}

	pub fn stats(&self) -> Arc<DaveStats> {
		self.stats.clone()
	}

	pub fn epoch(&self) -> Option<u64> {
		self.group
			.as_ref()
			.filter(|_| self.established)
			.map(|group| group.epoch().as_u64())
	}

	pub fn members(&self) -> Vec<u64> {
		self.group
			.as_ref()
			.filter(|_| self.established)
			.and_then(|group| roster(group).ok())
			.unwrap_or_default()
	}

	pub fn take_events(&mut self) -> Vec<DaveEvent> {
		std::mem::take(&mut self.events)
	}

	pub fn user_joined(&mut self, user_id: u64) {
		self.recognized.insert(user_id);
	}

	pub fn user_left(&mut self, user_id: u64) {
		if user_id != self.user_id {
			self.recognized.remove(&user_id);
		}
	}

	pub fn on_protocol_version(&mut self, version: u8) -> Vec<Outgoing> {
		self.crypto.lock().reset(version > 0);
		if version == 0 {
			self.announced_version = 0;
			self.discard_group();
			return Vec::new();
		}
		self.restart(version)
	}

	pub fn on_json(&mut self, op: u8, data: &Value) -> Vec<Outgoing> {
		match op {
			opcode::DAVE_PREPARE_TRANSITION => PrepareTransition::deserialize(data)
				.map(|event| self.on_prepare_transition(event.transition_id, event.protocol_version))
				.unwrap_or_default(),
			opcode::DAVE_EXECUTE_TRANSITION => {
				if let Ok(event) = ExecuteTransition::deserialize(data) {
					self.on_execute_transition(event.transition_id);
				}
				Vec::new()
			}
			opcode::DAVE_PREPARE_EPOCH => PrepareEpoch::deserialize(data)
				.map(|event| self.on_prepare_epoch(event.protocol_version, event.epoch))
				.unwrap_or_default(),
			_ => Vec::new(),
		}
	}

	pub fn on_binary(&mut self, op: u8, payload: &[u8]) -> Vec<Outgoing> {
		match op {
			opcode::DAVE_MLS_EXTERNAL_SENDER => {
				if let Err(error) = self.on_external_sender(payload) {
					self.report(&error);
				}
				Vec::new()
			}
			opcode::DAVE_MLS_PROPOSALS => match self.on_proposals(payload) {
				Ok(outgoing) => outgoing.into_iter().collect(),
				Err(error) => {
					self.report(&error);
					Vec::new()
				}
			},
			opcode::DAVE_MLS_ANNOUNCE_COMMIT => match split_transition(payload) {
				Some((transition_id, commit)) => self.on_announce_commit(transition_id, commit),
				None => Vec::new(),
			},
			opcode::DAVE_MLS_WELCOME => match split_transition(payload) {
				Some((transition_id, welcome)) => self.on_welcome(transition_id, welcome),
				None => Vec::new(),
			},
			_ => Vec::new(),
		}
	}

	fn on_external_sender(&mut self, payload: &[u8]) -> Result<()> {
		if self.established {
			return Err(VoiceError::Dave(
				"external sender changed after joining the group".to_owned(),
			));
		}
		let external_sender =
			ExternalSender::tls_deserialize_exact_bytes(payload).map_err(failure("external sender"))?;
		self.external_sender = Some(external_sender);
		if self.announced_version > 0 {
			self.create_pending_group()?;
		}
		Ok(())
	}

	fn on_proposals(&mut self, payload: &[u8]) -> Result<Option<Outgoing>> {
		let Self {
			provider,
			group,
			signer,
			recognized,
			..
		} = self;
		let Some(group) = group.as_mut() else {
			return Ok(None);
		};
		group
			.clear_pending_commit(provider.storage())
			.map_err(failure("clearing the pending commit"))?;
		let (&operation, body) = payload
			.split_first()
			.ok_or_else(|| VoiceError::Dave("empty proposals message".to_owned()))?;
		let list = VLBytes::tls_deserialize_exact_bytes(body).map_err(failure("proposal list"))?;
		let mut rest = list.as_slice();
		while !rest.is_empty() {
			rest = match operation {
				APPEND_PROPOSALS => {
					let (message, remainder) =
						MlsMessageIn::tls_deserialize_bytes(rest).map_err(failure("proposal"))?;
					queue_proposal(provider, group, recognized, message)?;
					remainder
				}
				REVOKE_PROPOSALS => {
					let (reference, remainder) =
						ProposalRef::tls_deserialize_bytes(rest).map_err(failure("proposal reference"))?;
					group
						.remove_pending_proposal(provider.storage(), &reference)
						.map_err(failure("revoking a proposal"))?;
					remainder
				}
				_ => return Err(VoiceError::Dave("unknown proposals operation".to_owned())),
			};
		}
		if !group.has_pending_proposals() {
			return Ok(None);
		}
		let (commit, welcome, _) = group
			.commit_to_pending_proposals(provider, signer)
			.map_err(failure("committing proposals"))?;
		let commit = commit.tls_serialize_detached().map_err(failure("commit"))?;
		let welcome = welcome
			.and_then(|message| match message.body() {
				MlsMessageBodyOut::Welcome(welcome) => Some(welcome.tls_serialize_detached()),
				_ => None,
			})
			.transpose()
			.map_err(failure("welcome"))?;
		Ok(Some(Outgoing::CommitWelcome { commit, welcome }))
	}

	fn on_announce_commit(&mut self, transition_id: u16, commit: &[u8]) -> Vec<Outgoing> {
		match self.process_commit(commit) {
			Ok(true) => self.stage(transition_id),
			Ok(false) => Vec::new(),
			Err(error) => self.fail(transition_id, &error),
		}
	}

	fn on_welcome(&mut self, transition_id: u16, welcome: &[u8]) -> Vec<Outgoing> {
		match self.process_welcome(welcome) {
			Ok(()) => self.stage(transition_id),
			Err(error) => self.fail(transition_id, &error),
		}
	}

	fn on_prepare_transition(&mut self, transition_id: u16, version: u8) -> Vec<Outgoing> {
		let transition = if version == 0 {
			self.crypto.lock().allow_plain_receive();
			self.bare_transition(transition_id, version)
		} else {
			match self.derive_keys() {
				Ok(keys) => self.install(transition_id, version, keys),
				Err(_) => self.bare_transition(transition_id, version),
			}
		};
		self.schedule(transition)
	}

	fn on_execute_transition(&mut self, transition_id: u16) {
		if let Some(transition) = self.pending.take_if(|pending| pending.id == transition_id) {
			self.apply(transition);
		}
	}

	fn on_prepare_epoch(&mut self, version: u8, epoch: u64) -> Vec<Outgoing> {
		if version == 0 {
			return Vec::new();
		}
		if epoch == FIRST_EPOCH {
			return self.restart(version);
		}
		self.announced_version = version;
		Vec::new()
	}

	fn process_commit(&mut self, commit: &[u8]) -> Result<bool> {
		let Self {
			provider,
			group,
			established,
			..
		} = self;
		let Some(group) = group.as_mut() else {
			return Ok(false);
		};
		let message = MlsMessageIn::tls_deserialize_exact_bytes(commit).map_err(failure("commit"))?;
		let protocol = message
			.try_into_protocol_message()
			.map_err(failure("commit framing"))?;
		if !matches!(protocol, ProtocolMessage::PublicMessage(_)) {
			return Err(VoiceError::Dave("commit must be a public message".to_owned()));
		}
		if protocol.group_id() != group.group_id() {
			return Err(VoiceError::Dave("commit for another group".to_owned()));
		}
		let epoch_before = group.epoch();
		let processed = match group.process_message(provider, protocol) {
			Ok(processed) => processed,
			Err(_) if !*established => return Ok(false),
			Err(error) => return Err(failure("processing the commit")(error)),
		};
		match processed.into_content() {
			ProcessedMessageContent::OwnPendingCommit => group
				.merge_pending_commit(provider)
				.map_err(failure("merging our commit"))?,
			ProcessedMessageContent::StagedCommitMessage(staged) => {
				if !*established {
					return Ok(false);
				}
				if staged.self_removed() {
					return Err(VoiceError::Dave("removed from the group".to_owned()));
				}
				for queued in staged.queued_proposals() {
					let allowed = queued.proposal_or_ref_type() == ProposalOrRefType::Reference
						&& matches!(queued.proposal(), Proposal::Add(_) | Proposal::Remove(_));
					if !allowed {
						return Err(VoiceError::Dave(
							"commit carries an unexpected proposal".to_owned(),
						));
					}
				}
				group
					.merge_staged_commit(provider, *staged)
					.map_err(failure("merging the commit"))?;
			}
			_ => return Err(VoiceError::Dave("message is not a commit".to_owned())),
		}
		if group.epoch().as_u64() != epoch_before.as_u64() + 1 {
			return Err(VoiceError::Dave("commit did not advance the epoch".to_owned()));
		}
		roster(group)?;
		*established = true;
		Ok(true)
	}

	fn process_welcome(&mut self, welcome: &[u8]) -> Result<()> {
		if self.established {
			return Err(VoiceError::Dave(
				"welcome received while already in a group".to_owned(),
			));
		}
		let external_sender = self
			.external_sender
			.as_ref()
			.ok_or_else(|| VoiceError::Dave("welcome received without an external sender".to_owned()))?;
		let welcome = Welcome::tls_deserialize_exact_bytes(welcome).map_err(failure("welcome"))?;
		let config = MlsGroupJoinConfig::builder()
			.use_ratchet_tree_extension(true)
			.wire_format_policy(PURE_PLAINTEXT_WIRE_FORMAT_POLICY)
			.build();
		let staged = StagedWelcome::build_from_welcome(&self.provider, &config, welcome)
			.map_err(failure("joining the group"))?
			.replace_old_group()
			.build()
			.map_err(failure("joining the group"))?;
		let context = staged.group_context();
		if context.group_id() != &self.group_id {
			return Err(VoiceError::Dave("welcome for another group".to_owned()));
		}
		let senders = context
			.extensions()
			.external_senders()
			.ok_or_else(|| VoiceError::Dave("group has no external sender".to_owned()))?;
		if senders.as_slice() != std::slice::from_ref(external_sender) {
			return Err(VoiceError::Dave(
				"group has an unexpected external sender".to_owned(),
			));
		}
		let mut members = HashSet::new();
		for member in staged.members() {
			let identity = identity(&member.credential)?;
			if !self.recognized.contains(&identity) || !members.insert(identity) {
				return Err(VoiceError::Dave("welcome lists an unexpected member".to_owned()));
			}
		}
		let group = staged
			.into_group(&self.provider)
			.map_err(failure("joining the group"))?;
		self.group = Some(group);
		self.established = true;
		Ok(())
	}

	fn stage(&mut self, transition_id: u16) -> Vec<Outgoing> {
		match self.derive_keys() {
			Ok(keys) => {
				let version = self.announced_version.max(PROTOCOL_VERSION);
				let transition = self.install(transition_id, version, keys);
				self.schedule(transition)
			}
			Err(error) => self.fail(transition_id, &error),
		}
	}

	fn install(&self, id: u16, version: u8, keys: EpochKeys) -> Transition {
		self.crypto.lock().install_senders(keys.senders);
		Transition {
			id,
			version,
			own: Some(keys.own),
			epoch: keys.epoch,
			members: keys.members,
		}
	}

	fn bare_transition(&self, id: u16, version: u8) -> Transition {
		Transition {
			id,
			version,
			own: None,
			epoch: self.epoch().unwrap_or_default(),
			members: self.members(),
		}
	}

	fn schedule(&mut self, transition: Transition) -> Vec<Outgoing> {
		if transition.id == INITIAL_TRANSITION {
			self.apply(transition);
			return Vec::new();
		}
		let ready = Outgoing::TransitionReady(transition.id);
		self.pending = Some(transition);
		vec![ready]
	}

	fn apply(&mut self, transition: Transition) {
		{
			let mut crypto = self.crypto.lock();
			match (transition.version, transition.own) {
				(0, _) => crypto.stop_sending(),
				(_, Some(own)) => crypto.start_sending(own),
				(_, None) => crypto.block_sending(),
			}
		}
		self.events.push(DaveEvent::Epoch {
			epoch: transition.epoch,
			version: transition.version,
			members: transition.members,
		});
	}

	fn derive_keys(&self) -> Result<EpochKeys> {
		let group = self
			.group
			.as_ref()
			.filter(|_| self.established)
			.ok_or_else(|| VoiceError::Dave("no established group".to_owned()))?;
		let members = roster(group)?;
		let mut own = None;
		let mut senders = Vec::with_capacity(members.len());
		for &member in &members {
			let ratchet = self.ratchet_for(group, member)?;
			if member == self.user_id {
				own = Some(ratchet);
			} else {
				senders.push((member, ratchet));
			}
		}
		let own = own.ok_or_else(|| VoiceError::Dave("we are not a member of the group".to_owned()))?;
		Ok(EpochKeys {
			own,
			senders,
			epoch: group.epoch().as_u64(),
			members,
		})
	}

	fn ratchet_for(&self, group: &MlsGroup, user_id: u64) -> Result<KeyRatchet> {
		let secret = group
			.export_secret(
				self.provider.crypto(),
				MEDIA_KEY_LABEL,
				&user_id.to_le_bytes(),
				KEY_BYTES,
			)
			.map_err(failure("exporting the sender key"))?;
		let base = secret
			.try_into()
			.map_err(|_| VoiceError::Dave("exported key has the wrong length".to_owned()))?;
		Ok(KeyRatchet::new(base))
	}

	fn restart(&mut self, version: u8) -> Vec<Outgoing> {
		self.announced_version = version;
		self.provider = OpenMlsRustCrypto::default();
		self.group = None;
		self.established = false;
		self.pending = None;
		let mut outgoing = Vec::new();
		if self.external_sender.is_some()
			&& let Err(error) = self.create_pending_group()
		{
			self.report(&error);
		}
		match self.key_package() {
			Ok(key_package) => outgoing.push(Outgoing::KeyPackage(key_package)),
			Err(error) => self.report(&error),
		}
		outgoing
	}

	fn discard_group(&mut self) {
		self.provider = OpenMlsRustCrypto::default();
		self.group = None;
		self.established = false;
		self.pending = None;
	}

	fn fail(&mut self, transition_id: u16, error: &VoiceError) -> Vec<Outgoing> {
		self.report(error);
		let mut outgoing = vec![Outgoing::InvalidCommitWelcome(transition_id)];
		outgoing.extend(self.restart(self.announced_version.max(PROTOCOL_VERSION)));
		outgoing
	}

	fn report(&mut self, error: &VoiceError) {
		self.events.push(DaveEvent::Failure(error.to_string()));
	}

	fn key_package(&self) -> Result<Vec<u8>> {
		let bundle = KeyPackage::builder()
			.key_package_extensions(Extensions::empty())
			.leaf_node_capabilities(capabilities())
			.key_package_lifetime(unbounded_lifetime())
			.build(SUITE, &self.provider, &self.signer, self.credential.clone())
			.map_err(failure("key package"))?;
		bundle
			.key_package()
			.tls_serialize_detached()
			.map_err(failure("key package"))
	}

	fn create_pending_group(&mut self) -> Result<()> {
		let external_sender = self
			.external_sender
			.clone()
			.ok_or_else(|| VoiceError::Dave("no external sender".to_owned()))?;
		if let Some(mut stale) = self.group.take() {
			stale
				.delete(self.provider.storage())
				.map_err(failure("dropping the pending group"))?;
		}
		let config = MlsGroupCreateConfig::builder()
			.ciphersuite(SUITE)
			.capabilities(capabilities())
			.lifetime(unbounded_lifetime())
			.with_group_context_extensions(
				Extensions::single(Extension::ExternalSenders(vec![external_sender]))
					.map_err(failure("group extensions"))?,
			)
			.use_ratchet_tree_extension(true)
			.wire_format_policy(PURE_PLAINTEXT_WIRE_FORMAT_POLICY)
			.build();
		let group = MlsGroup::new_with_group_id(
			&self.provider,
			&self.signer,
			&config,
			self.group_id.clone(),
			self.credential.clone(),
		)
		.map_err(failure("creating the group"))?;
		self.group = Some(group);
		Ok(())
	}

	#[cfg(feature = "testing")]
	pub fn leaf_index(&self, user_id: u64) -> Option<u32> {
		let group = self.group.as_ref()?;
		group
			.members()
			.find(|member| identity(&member.credential).is_ok_and(|identity| identity == user_id))
			.map(|member| member.index.u32())
	}

	#[cfg(feature = "testing")]
	pub fn exporter_check(&self) -> Option<[u8; 16]> {
		let group = self.group.as_ref().filter(|_| self.established)?;
		group
			.export_secret(self.provider.crypto(), "oxidcord test check", &[], 16)
			.ok()?
			.try_into()
			.ok()
	}
}

fn failure<E: Display>(context: &'static str) -> impl FnOnce(E) -> VoiceError {
	move |error| VoiceError::Dave(format!("{context}: {error}"))
}

fn capabilities() -> Capabilities {
	Capabilities::builder()
		.versions(vec![ProtocolVersion::Mls10])
		.ciphersuites(vec![SUITE])
		.extensions(vec![])
		.proposals(vec![])
		.credentials(vec![CredentialType::Basic])
		.build()
}

fn unbounded_lifetime() -> Lifetime {
	Lifetime::init(0, u64::MAX)
}

fn identity(credential: &Credential) -> Result<u64> {
	if credential.credential_type() != CredentialType::Basic {
		return Err(VoiceError::Dave(
			"credential is not a basic credential".to_owned(),
		));
	}
	credential
		.serialized_content()
		.try_into()
		.map(u64::from_be_bytes)
		.map_err(|_| VoiceError::Dave("credential identity is not a user id".to_owned()))
}

fn roster(group: &MlsGroup) -> Result<Vec<u64>> {
	let mut members = Vec::new();
	for member in group.members() {
		let identity = identity(&member.credential)?;
		if members.contains(&identity) {
			return Err(VoiceError::Dave("group lists a member twice".to_owned()));
		}
		members.push(identity);
	}
	Ok(members)
}

fn queue_proposal(
	provider: &OpenMlsRustCrypto,
	group: &mut MlsGroup,
	recognized: &HashSet<u64>,
	message: MlsMessageIn,
) -> Result<()> {
	let protocol = message
		.try_into_protocol_message()
		.map_err(failure("proposal framing"))?;
	if !matches!(protocol, ProtocolMessage::PublicMessage(_)) {
		return Err(VoiceError::Dave("proposal must be a public message".to_owned()));
	}
	let processed = group
		.process_message(provider, protocol)
		.map_err(failure("processing a proposal"))?;
	if !matches!(processed.sender(), Sender::External(_)) {
		return Err(VoiceError::Dave(
			"proposal is not from the external sender".to_owned(),
		));
	}
	let ProcessedMessageContent::ProposalMessage(queued) = processed.into_content() else {
		return Err(VoiceError::Dave("message is not a proposal".to_owned()));
	};
	match queued.proposal() {
		Proposal::Add(add) => {
			let user_id = identity(add.key_package().leaf_node().credential())?;
			if !recognized.contains(&user_id) {
				return Err(VoiceError::Dave("proposal adds an unexpected user".to_owned()));
			}
		}
		Proposal::Remove(_) => {}
		_ => return Err(VoiceError::Dave("proposal type is not allowed".to_owned())),
	}
	group
		.store_pending_proposal(provider.storage(), *queued)
		.map_err(failure("queueing a proposal"))
}

fn split_transition(payload: &[u8]) -> Option<(u16, &[u8])> {
	let (id, rest) = payload.split_first_chunk::<2>()?;
	Some((u16::from_be_bytes(*id), rest))
}
