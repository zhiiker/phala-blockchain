use alloc::vec::Vec;
use primitive_types::H256;
use core::hash::{Hash, Hasher};

use parity_scale_codec::{Decode, Encode};

pub type Path = Vec<u8>;
pub type SenderId = MessageOrigin;

/// The origin of a Phala message
// TODO: should we use XCM MultiLocation directly?
// [Reference](https://github.com/paritytech/xcm-format#multilocation-universal-destination-identifiers)
#[derive(Encode, Decode, Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum MessageOrigin {
    /// Runtime pallets (identified by pallet name)
    Pallet(Vec<u8>),
    /// A confidential contract
    Contract(H256),
    /// A pRuntime worker
    Worker(sp_core::ecdsa::Public),
    /// A user
    AccountId(H256),
    /// A remote location (parachain, etc.)
    MultiLocation(Vec<u8>),
    /// All gatekeepers share the same origin
    Gatekeeper,
}

impl Hash for MessageOrigin {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let encoded = Encode::encode(self);
        encoded.hash(state);
    }
}

impl MessageOrigin {
    /// Builds a new native confidential contract `MessageOrigin`
    pub fn native_contract(id: u32) -> Self {
        Self::Contract(H256::from_low_u64_be(id as u64))
    }

    /// Returns if the origin is located off-chain
    pub fn is_offchain(&self) -> bool {
        match self {
            Self::Contract(_) | Self::Worker(_) => true,
            _ => false,
        }
    }

    /// Returns if the origin is from a Pallet
    pub fn is_pallet(&self) -> bool {
        matches!(self, Self::Pallet(_))
    }
}

/// The topic in the message queue, indicating a group of destination message receivers.
///
/// A topic can be any non-empty binary string except there are some reserved value for the first byte.
///
/// # The reserved values for the first byte:
///
/// ~!@#$%&*_+-=|<>?,./;:'
///
/// # Indicator byte
///  Meaning of some special values appearing at the first byte:
///
///  - b'^': The topic's subscribers are on-chain only.
///
/// # Example:
/// ```rust
///    use phala_mq::Topic;
///
///    // An on-chain only topic. Messages sent to this topic will not be dispatched
///    // to off-chain components.
///    let an_onchain_topic = Topic::new(*b"^topic path");
///    assert!(!an_onchain_topic.is_offchain());
///
///    // An normal topic. Messages sent to this topic will be dispatched to off-chain subscribers
///    // as well as on-chain ones.
///    let a_normal_topic = Topic::new(*b"topic path");
///    assert!(a_normal_topic.is_offchain());
/// ```
///
#[derive(Encode, Decode, Clone, Eq, PartialEq, Hash)]
pub struct Topic(Path);

impl core::fmt::Debug for Topic {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let repr = alloc::string::String::from_utf8_lossy(&self.0[..]);
        f.write_str(repr.as_ref())
    }
}

impl Topic {
    const RESERVED_BYTES: &'static [u8] = b"~!@#$%&*_+-=|<>?,./;:'";

    pub fn new(path: impl Into<Path>) -> Self {
        Self(path.into())
    }

    pub fn path(&self) -> &Path {
        &self.0
    }

    pub fn is_offchain(&self) -> bool {
        if !self.is_valid() {
            return false;
        }
        self.0[0] != b'^'
    }

    pub fn is_valid(&self) -> bool {
        if self.0.is_empty() {
            return false;
        }
        !Self::RESERVED_BYTES.contains(&self.0[0])
    }
}

impl From<Path> for Topic {
    fn from(path: Path) -> Self {
        Self::new(path)
    }
}

impl From<Topic> for Path {
    fn from(topic: Topic) -> Self {
        topic.0
    }
}

/// Messages implementing BindTopic can be sent without giving the destination.
pub trait BindTopic {
    const TOPIC: &'static [u8];
}

impl BindTopic for () {
    const TOPIC: &'static [u8] = b"";
}

#[macro_export]
macro_rules! bind_topic {
    ($t: ident, $path: expr) => {
        impl $crate::types::BindTopic for $t {
            const TOPIC: &'static [u8] = $path;
        }
    };
    ($t: ident<$($gt: ident),+>, $path: expr) => {
        impl<$($gt),+> $crate::types::BindTopic for $t<$($gt),+> {
            const TOPIC: &'static [u8] = $path;
        }
    }
}

#[derive(Encode, Decode, Debug, Clone, Eq, PartialEq)]
pub struct Message {
    pub sender: SenderId,
    pub destination: Topic,
    pub payload: Vec<u8>,
}

impl Message {
    pub fn new(
        sender: impl Into<SenderId>,
        destination: impl Into<Path>,
        payload: Vec<u8>,
    ) -> Self {
        Message {
            sender: sender.into(),
            destination: Topic::new(destination),
            payload,
        }
    }

    pub fn decode_payload<T: Decode>(&self) -> Option<T> {
        Decode::decode(&mut &self.payload[..]).ok()
    }

    pub fn decode<T: Decode>(&self) -> Option<DecodedMessage<T>> {
        let payload = Decode::decode(&mut &self.payload[..]).ok()?;
        Some(DecodedMessage {
            sender: self.sender.clone(),
            destination: self.destination.clone(),
            payload
        })
    }
}

pub struct DecodedMessage<T> {
    pub sender: SenderId,
    pub destination: Topic,
    pub payload: T,
}

#[derive(Encode, Decode, Debug, Clone, Eq, PartialEq)]
pub struct SignedMessage {
    pub message: Message,
    pub sequence: u64,
    pub signature: Vec<u8>,
}

impl SignedMessage {
    pub fn data_be_signed(&self) -> Vec<u8> {
        MessageToBeSigned {
            message: &self.message,
            sequence: self.sequence,
        }
        .raw_data()
    }
}

#[derive(Encode)]
pub(crate) struct MessageToBeSigned<'a> {
    pub(crate) message: &'a Message,
    pub(crate) sequence: u64,
}

impl<'a> MessageToBeSigned<'a> {
    pub(crate) fn raw_data(&self) -> Vec<u8> {
        self.encode()
    }
}
