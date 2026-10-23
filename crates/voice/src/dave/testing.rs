use std::collections::{HashMap, HashSet, VecDeque};

use openmls::prelude::{
	BasicCredential, ExternalProposal, ExternalSender, GroupEpoch, GroupId, KeyPackageIn, LeafNodeIndex,
	MlsMessageIn, OpenMlsProvider, ProtocolVersion, SenderExtensionIndex, VLBytes,
};
use openmls_basic_credential::SignatureKeyPair;
use openmls_rust_crypto::OpenMlsRustCrypto;
use serde::Deserialize;
use serde_json::{Value, json};
use tls_codec::{DeserializeBytes, Serialize};
use tokio_tungstenite::tungstenite::Message;

use super::session::{DaveSession, Outgoing, PROTOCOL_VERSION, SUITE};
use super::transform::DaveTransform;
use crate::error::{Result, VoiceError};
use crate::wire::{ClientDisconnect, ClientsConnect, RawFrame, opcode};

const GATEWAY_IDENTITY: &[u8] = b"oxidcord test gateway";
const BINARY_HEADER_BYTES: usize = 3;
const ADD_OPERATION: u8 = 0;

pub struct Member {
	pub user_id: u64,
	pub session: DaveSession,
	pub transform: DaveTransform,
}

impl Member {
	pub fn new(user_id: u64, channel_id: u64) -> Result<Self> {
		let session = DaveSession::new(user_id, channel_id)?;
		let transform = session.transform(|ssrc| Some(u64::from(ssrc)));
		Ok(Self {
			user_id,
			session,
			transform,
		})
	}

	pub fn start(&mut self) -> Vec<Message> {
		let outgoing = self.session.on_protocol_version(PROTOCOL_VERSION);
		outgoing.iter().map(|outgoing| outgoing.encode()).collect()
	}

	pub fn receive(&mut self, message: &Message) -> Vec<Message> {
		let outgoing = match message {
			Message::Binary(bytes) if bytes.len() >= BINARY_HEADER_BYTES => {
				self.session.on_binary(bytes[2], &bytes[BINARY_HEADER_BYTES..])
			}
			Message::Text(text) => match serde_json::from_str::<RawFrame>(text.as_str()) {
				Ok(frame) => self.on_json(frame.op, &frame.d),
				Err(_) => Vec::new(),
			},
			_ => Vec::new(),
		};
		outgoing.iter().map(|outgoing| outgoing.encode()).collect()
	}

	fn on_json(&mut self, op: u8, data: &Value) -> Vec<Outgoing> {
		match op {
			opcode::CLIENTS_CONNECT => {
				if let Ok(event) = ClientsConnect::deserialize(data) {
					for user_id in event.user_ids {
						self.session.user_joined(user_id.get());
					}
				}
				Vec::new()
			}
			opcode::CLIENT_DISCONNECT => {
				if let Ok(event) = ClientDisconnect::deserialize(data) {
					self.session.user_left(event.user_id.get());
				}
				Vec::new()
			}
			_ => self.session.on_json(op, data),
		}
	}
}

struct Transition {
	id: u16,
	participants: Vec<u64>,
	ready: HashSet<u64>,
}

pub struct Gateway {
	crypto: OpenMlsRustCrypto,
	signer: SignatureKeyPair,
	group_id: GroupId,
	epoch: u64,
	transition_id: u16,
	sequences: HashMap<u64, u16>,
	connected: Vec<u64>,
	established: Vec<u64>,
	adds: Vec<(u64, Vec<u8>)>,
	removals: Vec<u64>,
	transition: Option<Transition>,
}

pub type Mail = Vec<(u64, Message)>;

impl Gateway {
	pub fn new(channel_id: u64) -> Result<Self> {
		let signer = SignatureKeyPair::new(SUITE.signature_algorithm())
			.map_err(|error| VoiceError::Dave(error.to_string()))?;
		Ok(Self {
			crypto: OpenMlsRustCrypto::default(),
			signer,
			group_id: GroupId::from_slice(&channel_id.to_be_bytes()),
			epoch: 0,
			transition_id: 0,
			sequences: HashMap::new(),
			connected: Vec::new(),
			established: Vec::new(),
			adds: Vec::new(),
			removals: Vec::new(),
			transition: None,
		})
	}

	pub fn epoch(&self) -> u64 {
		self.epoch
	}

	pub fn join(&mut self, user_id: u64) -> Mail {
		let mut mail = Mail::new();
		let external_sender = self.external_sender();
		mail.push((
			user_id,
			self.binary(user_id, opcode::DAVE_MLS_EXTERNAL_SENDER, &external_sender),
		));
		if !self.connected.is_empty() {
			mail.push((user_id, clients_connect(&self.connected)));
		}
		if self.established.is_empty() {
			for (_, proposal) in self.adds.clone() {
				mail.push((user_id, self.proposals(user_id, &proposal)));
			}
		}
		for existing in self.connected.clone() {
			mail.push((existing, clients_connect(&[user_id])));
		}
		self.connected.push(user_id);
		mail
	}

	pub fn leave(&mut self, user_id: u64, leaf_index: u32) -> Result<Mail> {
		self.connected.retain(|user| *user != user_id);
		self.adds.retain(|(user, _)| *user != user_id);
		let mut mail = Mail::new();
		for user in self.connected.clone() {
			mail.push((user, client_disconnect(user_id)));
		}
		if !self.established.contains(&user_id) {
			return Ok(mail);
		}
		self.removals.push(user_id);
		let proposal = ExternalProposal::new_remove::<OpenMlsRustCrypto>(
			LeafNodeIndex::new(leaf_index),
			self.group_id.clone(),
			GroupEpoch::from(self.epoch),
			&self.signer,
			SenderExtensionIndex::new(0),
		)
		.map_err(|error| VoiceError::Dave(error.to_string()))?
		.tls_serialize_detached()
		.map_err(|error| VoiceError::Dave(error.to_string()))?;
		for user in self.established.clone() {
			if user != user_id {
				mail.push((user, self.proposals(user, &proposal)));
			}
		}
		Ok(mail)
	}

	pub fn receive(&mut self, from: u64, message: &Message) -> Result<Mail> {
		match message {
			Message::Binary(bytes) => match bytes.split_first() {
				Some((&opcode::DAVE_MLS_KEY_PACKAGE, payload)) => self.on_key_package(from, payload),
				Some((&opcode::DAVE_MLS_COMMIT_WELCOME, payload)) => self.on_commit_welcome(from, payload),
				_ => Ok(Mail::new()),
			},
			Message::Text(text) => {
				let frame = serde_json::from_str::<RawFrame>(text.as_str())
					.map_err(|error| VoiceError::Dave(error.to_string()))?;
				if frame.op == opcode::DAVE_TRANSITION_READY {
					let id = frame.d.get("transition_id").and_then(Value::as_u64);
					return Ok(self.on_ready(from, id));
				}
				Ok(Mail::new())
			}
			_ => Ok(Mail::new()),
		}
	}

	fn external_sender(&self) -> Vec<u8> {
		ExternalSender::new(
			self.signer.public().into(),
			BasicCredential::new(GATEWAY_IDENTITY.to_vec()).into(),
		)
		.tls_serialize_detached()
		.expect("external sender serializes")
	}

	fn on_key_package(&mut self, from: u64, payload: &[u8]) -> Result<Mail> {
		let key_package = KeyPackageIn::tls_deserialize_exact_bytes(payload)
			.map_err(|error| VoiceError::Dave(error.to_string()))?
			.validate(self.crypto.crypto(), ProtocolVersion::Mls10)
			.map_err(|error| VoiceError::Dave(error.to_string()))?;
		let proposal = ExternalProposal::new_add::<OpenMlsRustCrypto>(
			key_package,
			self.group_id.clone(),
			GroupEpoch::from(self.epoch),
			&self.signer,
			SenderExtensionIndex::new(0),
		)
		.map_err(|error| VoiceError::Dave(error.to_string()))?
		.tls_serialize_detached()
		.map_err(|error| VoiceError::Dave(error.to_string()))?;
		self.adds.retain(|(user, _)| *user != from);
		self.adds.push((from, proposal.clone()));
		let recipients = if self.established.is_empty() {
			self.connected.clone()
		} else {
			self.established.clone()
		};
		Ok(recipients
			.into_iter()
			.filter(|user| *user != from)
			.map(|user| (user, self.proposals(user, &proposal)))
			.collect())
	}

	fn on_commit_welcome(&mut self, from: u64, payload: &[u8]) -> Result<Mail> {
		let (commit, welcome) = MlsMessageIn::tls_deserialize_bytes(payload)
			.map_err(|error| VoiceError::Dave(error.to_string()))?;
		let epoch = commit
			.try_into_protocol_message()
			.map_err(|error| VoiceError::Dave(error.to_string()))?
			.epoch()
			.as_u64();
		if epoch != self.epoch {
			return Ok(Mail::new());
		}
		let commit_bytes = &payload[..payload.len() - welcome.len()];
		self.epoch += 1;
		self.transition_id += 1;
		let id = self.transition_id;
		let mut recipients = self.established.clone();
		if !recipients.contains(&from) {
			recipients.push(from);
		}
		let welcomed: Vec<u64> = self
			.adds
			.iter()
			.map(|(user, _)| *user)
			.filter(|user| *user != from && self.connected.contains(user))
			.collect();
		let mut participants: Vec<u64> = recipients
			.iter()
			.copied()
			.filter(|user| !self.removals.contains(user))
			.collect();
		participants.extend(welcomed.iter().copied());
		self.established = participants.clone();
		self.adds.clear();
		self.removals.clear();
		self.transition = Some(Transition {
			id,
			participants,
			ready: HashSet::new(),
		});
		let mut mail = Mail::new();
		for user in recipients {
			if self.connected.contains(&user) {
				mail.push((user, self.announce(user, id, commit_bytes)));
			}
		}
		for user in welcomed {
			mail.push((user, self.welcome(user, id, welcome)));
		}
		Ok(mail)
	}

	fn on_ready(&mut self, from: u64, id: Option<u64>) -> Mail {
		let Some(transition) = self.transition.as_mut() else {
			return Mail::new();
		};
		if id != Some(u64::from(transition.id)) {
			return Mail::new();
		}
		transition.ready.insert(from);
		let done = transition
			.participants
			.iter()
			.all(|user| transition.ready.contains(user));
		if !done {
			return Mail::new();
		}
		let transition = self.transition.take().expect("transition is present");
		transition
			.participants
			.into_iter()
			.map(|user| (user, execute(transition.id)))
			.collect()
	}

	fn next_sequence(&mut self, user_id: u64) -> u16 {
		let sequence = self.sequences.entry(user_id).or_default();
		let current = *sequence;
		*sequence = sequence.wrapping_add(1);
		current
	}

	fn binary(&mut self, user_id: u64, op: u8, payload: &[u8]) -> Message {
		let mut bytes = Vec::with_capacity(BINARY_HEADER_BYTES + payload.len());
		bytes.extend_from_slice(&self.next_sequence(user_id).to_be_bytes());
		bytes.push(op);
		bytes.extend_from_slice(payload);
		Message::binary(bytes)
	}

	fn proposals(&mut self, user_id: u64, proposal: &[u8]) -> Message {
		let list = VLBytes::new(proposal.to_vec())
			.tls_serialize_detached()
			.expect("proposal list serializes");
		let mut payload = vec![ADD_OPERATION];
		payload.extend_from_slice(&list);
		self.binary(user_id, opcode::DAVE_MLS_PROPOSALS, &payload)
	}

	fn announce(&mut self, user_id: u64, id: u16, commit: &[u8]) -> Message {
		let mut payload = id.to_be_bytes().to_vec();
		payload.extend_from_slice(commit);
		self.binary(user_id, opcode::DAVE_MLS_ANNOUNCE_COMMIT, &payload)
	}

	fn welcome(&mut self, user_id: u64, id: u16, welcome: &[u8]) -> Message {
		let mut payload = id.to_be_bytes().to_vec();
		payload.extend_from_slice(welcome);
		self.binary(user_id, opcode::DAVE_MLS_WELCOME, &payload)
	}
}

fn clients_connect(user_ids: &[u64]) -> Message {
	let ids: Vec<String> = user_ids.iter().map(u64::to_string).collect();
	Message::text(json!({ "op": opcode::CLIENTS_CONNECT, "d": { "user_ids": ids } }).to_string())
}

fn client_disconnect(user_id: u64) -> Message {
	Message::text(
		json!({ "op": opcode::CLIENT_DISCONNECT, "d": { "user_id": user_id.to_string() } }).to_string(),
	)
}

fn execute(id: u16) -> Message {
	Message::text(json!({ "op": opcode::DAVE_EXECUTE_TRANSITION, "d": { "transition_id": id } }).to_string())
}

enum Event {
	ToGateway(u64, Message),
	ToUser(u64, Message),
}

pub struct Harness {
	pub gateway: Gateway,
	channel_id: u64,
	members: HashMap<u64, Member>,
	departed: HashMap<u64, Member>,
	outboxes: HashMap<u64, VecDeque<Message>>,
}

impl Harness {
	pub fn new(channel_id: u64) -> Result<Self> {
		Ok(Self {
			gateway: Gateway::new(channel_id)?,
			channel_id,
			members: HashMap::new(),
			departed: HashMap::new(),
			outboxes: HashMap::new(),
		})
	}

	pub fn join(&mut self, user_id: u64) -> Result<()> {
		let mut member = Member::new(user_id, self.channel_id)?;
		let mut events: VecDeque<Event> = self
			.gateway
			.join(user_id)
			.into_iter()
			.map(|(to, message)| Event::ToUser(to, message))
			.collect();
		for message in member.start() {
			events.push_back(Event::ToGateway(user_id, message));
		}
		self.members.insert(user_id, member);
		self.run(events)
	}

	pub fn join_external(&mut self, user_id: u64) -> Result<()> {
		self.outboxes.insert(user_id, VecDeque::new());
		let events = self
			.gateway
			.join(user_id)
			.into_iter()
			.map(|(to, message)| Event::ToUser(to, message))
			.collect();
		self.run(events)
	}

	pub fn deliver(&mut self, from: u64, message: Message) -> Result<()> {
		self.run(VecDeque::from([Event::ToGateway(from, message)]))
	}

	pub fn take_outbox(&mut self, user_id: u64) -> Vec<Message> {
		self.outboxes
			.get_mut(&user_id)
			.map(|outbox| outbox.drain(..).collect())
			.unwrap_or_default()
	}

	pub fn leave(&mut self, user_id: u64) -> Result<()> {
		let leaf_index = self
			.members
			.values()
			.find_map(|member| member.session.leaf_index(user_id))
			.ok_or_else(|| VoiceError::Dave("no leaf index for the leaving user".to_owned()))?;
		if let Some(member) = self.members.remove(&user_id) {
			self.departed.insert(user_id, member);
		}
		let events = self
			.gateway
			.leave(user_id, leaf_index)?
			.into_iter()
			.map(|(to, message)| Event::ToUser(to, message))
			.collect();
		self.run(events)
	}

	pub fn member(&mut self, user_id: u64) -> Option<&mut Member> {
		self.members.get_mut(&user_id)
	}

	pub fn departed(&mut self, user_id: u64) -> Option<&mut Member> {
		self.departed.get_mut(&user_id)
	}

	fn run(&mut self, mut events: VecDeque<Event>) -> Result<()> {
		while let Some(event) = events.pop_front() {
			match event {
				Event::ToGateway(from, message) => {
					for (to, message) in self.gateway.receive(from, &message)? {
						events.push_back(Event::ToUser(to, message));
					}
				}
				Event::ToUser(to, message) => {
					if let Some(outbox) = self.outboxes.get_mut(&to) {
						outbox.push_back(message);
					} else if let Some(member) = self.members.get_mut(&to) {
						for reply in member.receive(&message) {
							events.push_back(Event::ToGateway(to, reply));
						}
					}
				}
			}
		}
		Ok(())
	}
}
