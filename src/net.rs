// Copyright (c) 2019 Chaintope Inc.
// Distributed under the MIT software license, see the accompanying
// file COPYING or http://www.opensource.org/licenses/mit-license.php.

use crate::blockdata::Block;
use crate::errors;
use crate::serialize::ByteBufVisitor;
use bitcoin::PublicKey;
use redis::{Client, Commands, ControlFlow, PubSubCommands, RedisError};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::cmp::Ordering;
use std::sync::mpsc::{channel, Receiver, Sender};
/// メッセージを受け取って、それを処理するためのモジュール
/// メッセージの処理は、メッセージの種類とラウンドの状態に依存する。
/// ラウンドの状態は 誰が master であるか（自身がmaster であるか）。ラウンドが実行中であるか、開始待ちであるか。などで変わる
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use curv::cryptographic_primitives::secret_sharing::feldman_vss::VerifiableSS;
use curv::FE;

/// Signerの識別子。公開鍵を識別子にする。
#[derive(Debug, PartialEq, Eq, Hash, Copy, Clone)]
pub struct SignerID {
    pub pubkey: PublicKey,
}

impl SignerID {
    pub fn new(pubkey: PublicKey) -> Self {
        SignerID { pubkey }
    }
}

impl Serialize for SignerID {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use bitcoin::util::psbt::serialize::Serialize;

        let ser = self.pubkey.serialize();
        serializer.serialize_bytes(&ser[..])
    }
}

impl<'de> Deserialize<'de> for SignerID {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = deserializer.deserialize_byte_buf(ByteBufVisitor)?;

        // TODO: Handle when PublicKey::from_slice returns Error
        let pubkey = PublicKey::from_slice(&vec).unwrap();
        let signer_id = SignerID::new(pubkey);
        Ok(signer_id)
    }
}

#[derive(PartialEq, Debug, Serialize, Deserialize)]
pub enum MessageType {
    Candidateblock(Block),
    Completedblock(Block),
    Nodevss(VerifiableSS, FE),
    Blockvss(Block, VerifiableSS, FE),
    Blocksig(Block, FE, FE),
    Roundfailure,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Message {
    pub message_type: MessageType,
    pub sender_id: SignerID,
    pub receiver_id: Option<SignerID>,
}

#[derive(Debug, PartialEq)]
pub struct Signature(pub secp256k1::Signature);

impl Serialize for Signature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let ser = self.0.serialize_der();
        serializer.serialize_bytes(&ser[..])
    }
}

impl<'de> Deserialize<'de> for Signature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec = deserializer.deserialize_byte_buf(ByteBufVisitor)?;

        // TODO: handle parse error
        let signature = secp256k1::Signature::from_der(&vec).unwrap();
        Ok(Signature(signature))
    }
}

pub trait ConnectionManager {
    type ERROR: std::error::Error;
    fn broadcast_message(&self, message: Message);
    fn send_message(&self, message: Message);
    fn start(
        &self,
        message_processor: impl FnMut(Message) -> ControlFlow<()> + Send + 'static,
        id: SignerID,
    ) -> JoinHandle<()>;
    fn error_handler(&mut self) -> Option<Receiver<ConnectionManagerError<Self::ERROR>>>;
}

#[derive(Debug)]
pub struct ConnectionManagerError<E: std::error::Error> {
    description: String,
    cause: Option<E>,
}

impl<E: std::error::Error> std::fmt::Display for ConnectionManagerError<E> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl<E: std::error::Error> std::error::Error for ConnectionManagerError<E> {
    fn description(&self) -> &str {
        &self.description
    }

    fn cause(&self) -> Option<&dyn std::error::Error> {
        match self.cause {
            Some(ref e) => Some(e),
            None => None,
        }
    }
}

impl From<RedisError> for ConnectionManagerError<RedisError> {
    fn from(cause: RedisError) -> ConnectionManagerError<RedisError> {
        ConnectionManagerError {
            description: format!("{:?}", cause),
            cause: Some(cause),
        }
    }
}

pub struct RedisManager {
    pub client: Arc<Client>,
    error_sender: Sender<ConnectionManagerError<RedisError>>,
    pub error_receiver: Option<Receiver<ConnectionManagerError<RedisError>>>,
}

impl RedisManager {
    pub fn new(host: String, port: String) -> Self {
        let url: &str = &format!("redis://{}:{}", host, port);
        let client = Arc::new(Client::open(url).unwrap());
        let (s, r): (
            Sender<ConnectionManagerError<RedisError>>,
            Receiver<ConnectionManagerError<RedisError>>,
        ) = channel();
        RedisManager {
            client,
            error_sender: s,
            error_receiver: Some(r),
        }
    }

    pub fn test_connection(&self) -> Result<(), errors::Error> {
        match self.client.get_connection() {
            Ok(_) => Ok(()),
            Err(e) => Err(errors::Error::from(e)),
        }
    }

    fn subscribe<F>(&self, message_processor: F, id: SignerID) -> thread::JoinHandle<()>
    where
        F: FnMut(Message) -> ControlFlow<()> + Send + 'static,
    {
        let client = Arc::clone(&self.client);
        let error_sender = self.error_sender.clone();
        let channel_name = format!("tapyrus-signer-{}", id.pubkey.key);
        thread::Builder::new()
            .name("RedisManagerThread".to_string())
            .spawn(move || {
                fn inner_subscribe<F2>(
                    client: Arc<Client>,
                    mut message_processor: F2,
                    channel_name: &str,
                ) -> Result<(), ConnectionManagerError<RedisError>>
                where
                    F2: FnMut(Message) -> ControlFlow<()> + Send + 'static,
                {
                    let mut conn = client.get_connection()?;
                    conn.subscribe(&["tapyrus-signer", channel_name], |msg| {
                        let _ch = msg.get_channel_name();
                        let payload: String = msg.get_payload().unwrap();
                        log::trace!("receive message. payload: {}", payload);

                        let message: Message = serde_json::from_str(&payload).unwrap();
                        message_processor(message)
                    })?;
                    Ok(())
                }
                match inner_subscribe(client, message_processor, &channel_name) {
                    Ok(()) => {}
                    Err(e) => error_sender
                        .send(e)
                        .expect("Can't notify RedisManager connection error"),
                };
            })
            .expect("Failed create RedisManagerThread.")
    }

    fn process_message(&self, message: Message, to: String) {
        let client = Arc::clone(&self.client);
        let message_in_thread = serde_json::to_string(&message).unwrap();
        thread::Builder::new()
            .name("RedisBroadcastThread".to_string())
            .spawn(move || {
                let conn = client.get_connection().unwrap();
                thread::sleep(Duration::from_millis(500));

                log::trace!("Publish {} to tapyrus-signer channel.", message_in_thread);

                let _: () = conn.publish(to, message_in_thread).unwrap();
            })
            .unwrap()
            .join()
            .expect("Can't connect to Redis Server.");
    }
}

impl ConnectionManager for RedisManager {
    type ERROR = RedisError;

    fn broadcast_message(&self, message: Message) {
        assert!(message.receiver_id.is_none());
        log::debug!("broadcast_message {:?} ", message);
        let channel_name = "tapyrus-signer".to_string();
        self.process_message(message, channel_name);
    }

    fn send_message(&self, message: Message) {
        assert!(message.receiver_id.is_some());
        log::debug!("send_message {:?} ", message);
        let channel_name = format!("tapyrus-signer-{}", message.receiver_id.unwrap().pubkey.key);
        self.process_message(message, channel_name);
    }

    fn start(
        &self,
        message_processor: impl FnMut(Message) -> ControlFlow<()> + Send + 'static,
        id: SignerID,
    ) -> JoinHandle<()> {
        self.subscribe(message_processor, id)
    }

    fn error_handler(&mut self) -> Option<Receiver<ConnectionManagerError<Self::ERROR>>> {
        self.error_receiver.take()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::test_helper::TestKeys;

    #[test]
    #[ignore]
    fn redis_connection_test() {
        let connection_manager = Arc::new(RedisManager::new(
            "localhost".to_string(),
            "6379".to_string(),
        ));
        let sender_id = SignerID {
            pubkey: TestKeys::new().pubkeys()[0],
        };

        let message_processor = move |message: Message| {
            assert_eq!(message.message_type, MessageType::Roundfailure);
            ControlFlow::Break(())
        };

        let subscriber = connection_manager.subscribe(message_processor, sender_id);

        let message = Message {
            message_type: MessageType::Roundfailure,
            sender_id,
            receiver_id: None,
        };
        connection_manager.broadcast_message(message);

        subscriber.join().unwrap();
    }

    #[test]
    fn signer_id_serialize_test() {
        let pubkey = TestKeys::new().pubkeys()[0];
        let signer_id: SignerID = SignerID { pubkey };
        let serialized = serde_json::to_string(&signer_id).unwrap();
        assert_eq!("[3,131,26,105,184,0,152,51,171,91,3,38,1,46,175,72,155,254,163,90,115,33,177,202,21,177,29,136,19,20,35,250,252]", serialized);
    }

    #[test]
    fn signer_id_deserialize_test() {
        let serialized = "[3,131,26,105,184,0,152,51,171,91,3,38,1,46,175,72,155,254,163,90,115,33,177,202,21,177,29,136,19,20,35,250,252]";
        let signer_id = serde_json::from_str::<SignerID>(serialized).unwrap();

        let pubkey = TestKeys::new().pubkeys()[0];
        let expected: SignerID = SignerID { pubkey };
        assert_eq!(expected, signer_id);
    }
}
