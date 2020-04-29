use crate::blockdata::Block;
use crate::net::SignerID;
use crate::signer_node::BidirectionalSharedSecretMap;
use curv::{FE, GE};
use std::collections::{BTreeMap, HashSet};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NodeState {
    /// The node is trying to get joining signer network.
    /// All nodes start with Joining state when the node gets started.
    Joining,
    /// This state is set if the node is not a member of the current federation. This state is given
    /// when the node would join to the federation in future, or it was eliminated from the
    /// federation in past block height.
    Idling {
        /// A height at the block which is the next block of the tip.
        block_height: u64,
    },
    Master {
        /// *block_key* is random value for using int the Signature Issuing Protocol.
        /// VSS which is distributed to each other signer is generated by this key. All signers in
        /// all block generation rounds has each own block_key.
        block_key: Option<FE>,
        /// Map of VSSs and commitment in Signature Issuing Protocol. A Signer broadcasts this value
        /// on blockvss message and collected by all signers who include oneself.
        shared_block_secrets: BidirectionalSharedSecretMap,
        /// Share in which is generated from above shared_block_secrets. It is produced by
        /// aggregating VSSs and first element of the commitments which are come from the
        /// participants who are selected by round master.
        block_shared_keys: Option<(bool, FE, GE)>,
        /// Candidate block of a round.
        /// It is broadcasted by master node of a round. The goal of rounds are generating signature
        /// for this candidate block.
        candidate_block: Option<Block>,
        /// Map of local signatures for each signers. Final signature for candidate block is calculated by these
        /// signatures on lagrange interpolation.
        signatures: BTreeMap<SignerID, (FE, FE)>,
        /// The set of participants who can participate signature issuing protocol. The participants
        /// are declared by Master node of the round.
        participants: HashSet<SignerID>,
        /// Set true when the round is done.
        round_is_done: bool,
        /// A height of the block which is the next block of the tip. It means that it is the height of the block which is the round would create.
        block_height: u64,
    },
    Member {
        /// *block_key* is random value for using int the Signature Issuing Protocol.
        /// VSS which is distributed to each other signer is generated by this key. All signers in
        /// all block generation rounds has each own block_key.
        block_key: Option<FE>,
        /// Map of VSSs and commitment in Signature Issuing Protocol. A Signer broadcasts this value
        /// on blockvss message and collected by all signers who include oneself.
        shared_block_secrets: BidirectionalSharedSecretMap,
        /// Share in which is generated from above shared_block_secrets. It is produced by
        /// aggregating VSSs and first element of the commitments which are come from the
        /// participants who are selected by round master.
        block_shared_keys: Option<(bool, FE, GE)>,
        /// Candidate block of a round.
        /// It is broadcasted by master node of a round. The goal of rounds are generating signature
        /// for this candidate block.
        candidate_block: Option<Block>,
        /// The set of participants who can participate signature issuing protocol. The participants
        /// are declared by Master node of the round.
        participants: HashSet<SignerID>,
        master_index: usize,
        /// A height of the block which is the next block of the tip. It means that it is the height of the block which is the round would create.
        block_height: u64,
    },
    RoundComplete {
        master_index: usize,
        next_master_index: usize,
        block_height: u64,
    },
}

impl NodeState {
    pub fn block_height(&self) -> u64 {
        match &self {
            NodeState::Idling { block_height } => *block_height,
            NodeState::Master { block_height, .. } => *block_height,
            NodeState::Member { block_height, .. } => *block_height,
            NodeState::RoundComplete { block_height, .. } => *block_height,
            NodeState::Joining => unreachable!(),
        }
    }
}

pub mod builder {
    use crate::blockdata::Block;
    use crate::crypto::multi_party_schnorr::LocalSig;
    use crate::net::SignerID;
    use crate::signer_node::{
        BidirectionalSharedSecretMap, NodeState, SharedSecret, INITIAL_MASTER_INDEX,
    };
    use curv::{FE, GE};
    use std::borrow::BorrowMut;
    use std::collections::{BTreeMap, HashSet};

    pub trait Builder {
        fn build(&self) -> NodeState;
        fn from_node_state(state: NodeState) -> Self;
    }

    pub struct Master {
        block_key: Option<FE>,
        shared_block_secrets: BidirectionalSharedSecretMap,
        block_shared_keys: Option<(bool, FE, GE)>,
        candidate_block: Option<Block>,
        signatures: BTreeMap<SignerID, (FE, FE)>,
        participants: HashSet<SignerID>,
        round_is_done: bool,
        block_height: u64,
    }

    impl Builder for Master {
        fn build(&self) -> NodeState {
            NodeState::Master {
                block_key: self.block_key.clone(),
                shared_block_secrets: self.shared_block_secrets.clone(),
                block_shared_keys: self.block_shared_keys.clone(),
                candidate_block: self.candidate_block.clone(),
                signatures: self.signatures.clone(),
                participants: self.participants.clone(),
                round_is_done: self.round_is_done,
                block_height: 0,
            }
        }

        fn from_node_state(state: NodeState) -> Self {
            if let NodeState::Master {
                block_key,
                shared_block_secrets,
                block_shared_keys,
                candidate_block,
                signatures,
                participants,
                round_is_done,
                block_height,
            } = state
            {
                Self {
                    block_key,
                    shared_block_secrets,
                    block_shared_keys,
                    candidate_block,
                    signatures,
                    participants,
                    round_is_done,
                    block_height,
                }
            } else {
                unreachable!(
                    "builder::Master::from_node_state should receive NodeState::Master variant"
                );
            }
        }
    }

    impl Default for Master {
        fn default() -> Self {
            Self {
                block_key: None,
                shared_block_secrets: BidirectionalSharedSecretMap::new(),
                block_shared_keys: None,
                candidate_block: None,
                signatures: BTreeMap::new(),
                participants: HashSet::new(),
                round_is_done: false,
                block_height: 0,
            }
        }
    }

    impl Master {
        pub fn new(
            block_key: Option<FE>,
            shared_block_secrets: BidirectionalSharedSecretMap,
            block_shared_keys: Option<(bool, FE, GE)>,
            candidate_block: Option<Block>,
            signatures: BTreeMap<SignerID, (FE, FE)>,
            participants: HashSet<SignerID>,
            round_is_done: bool,
            block_height: u64,
        ) -> Self {
            Self {
                block_key,
                shared_block_secrets,
                block_shared_keys,
                candidate_block,
                signatures,
                participants,
                round_is_done,
                block_height,
            }
        }

        pub fn block_key(&mut self, block_key: Option<FE>) -> &mut Self {
            self.block_key = block_key;
            self
        }

        pub fn insert_shared_block_secrets(
            &mut self,
            signer_id: SignerID,
            shared_secret_for_positive: SharedSecret,
            shared_secret_for_negative: SharedSecret,
        ) -> &mut Self {
            self.shared_block_secrets.insert(
                signer_id,
                (shared_secret_for_positive, shared_secret_for_negative),
            );
            self
        }

        pub fn shared_block_secrets(
            &mut self,
            shared_block_secrets: BidirectionalSharedSecretMap,
        ) -> &mut Self {
            self.shared_block_secrets = shared_block_secrets;
            self
        }

        pub fn block_shared_keys(
            &mut self,
            block_shared_keys: Option<(bool, FE, GE)>,
        ) -> &mut Self {
            self.block_shared_keys = block_shared_keys;
            self
        }

        pub fn candidate_block(&mut self, candidate_block: Option<Block>) -> &mut Self {
            self.candidate_block = candidate_block;
            self
        }

        pub fn insert_signature(&mut self, signer_id: SignerID, local_sig: LocalSig) -> &mut Self {
            let LocalSig { gamma_i, e } = local_sig;
            self.signatures.insert(signer_id, (gamma_i, e));
            self
        }

        pub fn borrow_mut_signatures(&mut self) -> &mut BTreeMap<SignerID, (FE, FE)> {
            self.signatures.borrow_mut()
        }

        pub fn signatures(&mut self, signatures: BTreeMap<SignerID, (FE, FE)>) -> &mut Self {
            self.signatures = signatures;
            self
        }

        pub fn participants(&mut self, participants: HashSet<SignerID>) -> &mut Self {
            self.participants = participants;
            self
        }

        pub fn round_is_done(&mut self, round_is_done: bool) -> &mut Self {
            self.round_is_done = round_is_done;
            self
        }

        pub fn block_height(&mut self, block_height: u64) -> &mut Self {
            self.block_height = block_height;
            self
        }
    }

    pub struct Member {
        block_key: Option<FE>,
        shared_block_secrets: BidirectionalSharedSecretMap,
        block_shared_keys: Option<(bool, FE, GE)>,
        candidate_block: Option<Block>,
        participants: HashSet<SignerID>,
        master_index: usize,
        block_height: u64,
    }

    impl Default for Member {
        fn default() -> Self {
            Self {
                block_key: None,
                shared_block_secrets: BidirectionalSharedSecretMap::new(),
                block_shared_keys: None,
                candidate_block: None,
                participants: HashSet::new(),
                master_index: INITIAL_MASTER_INDEX,
                block_height: 0,
            }
        }
    }

    impl Builder for Member {
        fn build(&self) -> NodeState {
            NodeState::Member {
                block_key: self.block_key.clone(),
                shared_block_secrets: self.shared_block_secrets.clone(),
                block_shared_keys: self.block_shared_keys.clone(),
                candidate_block: self.candidate_block.clone(),
                participants: self.participants.clone(),
                master_index: self.master_index,
                block_height: self.block_height,
            }
        }

        fn from_node_state(state: NodeState) -> Self {
            if let NodeState::Member {
                block_key,
                shared_block_secrets,
                block_shared_keys,
                candidate_block,
                participants,
                master_index,
                block_height,
            } = state
            {
                Self {
                    block_key,
                    shared_block_secrets,
                    block_shared_keys,
                    candidate_block,
                    participants,
                    master_index,
                    block_height,
                }
            } else {
                unreachable!(
                    "builder::Member::from_node_state should receive NodeState::Member variant"
                );
            }
        }
    }

    impl Member {
        pub fn new(
            block_key: Option<FE>,
            shared_block_secrets: BidirectionalSharedSecretMap,
            block_shared_keys: Option<(bool, FE, GE)>,
            candidate_block: Option<Block>,
            participants: HashSet<SignerID>,
            master_index: usize,
            block_height: u64,
        ) -> Self {
            Self {
                block_key,
                shared_block_secrets,
                block_shared_keys,
                candidate_block,
                participants,
                master_index,
                block_height,
            }
        }

        pub fn block_key(&mut self, block_key: Option<FE>) -> &mut Self {
            self.block_key = block_key;
            self
        }

        pub fn insert_shared_block_secrets(
            &mut self,
            signer_id: SignerID,
            shared_secret_for_positive: SharedSecret,
            shared_secret_for_negative: SharedSecret,
        ) -> &mut Self {
            self.shared_block_secrets.insert(
                signer_id,
                (shared_secret_for_positive, shared_secret_for_negative),
            );
            self
        }

        pub fn shared_block_secrets(
            &mut self,
            shared_block_secrets: BidirectionalSharedSecretMap,
        ) -> &mut Self {
            self.shared_block_secrets = shared_block_secrets;
            self
        }

        pub fn block_shared_keys(
            &mut self,
            block_shared_keys: Option<(bool, FE, GE)>,
        ) -> &mut Self {
            self.block_shared_keys = block_shared_keys;
            self
        }

        pub fn candidate_block(&mut self, candidate_block: Option<Block>) -> &mut Self {
            self.candidate_block = candidate_block;
            self
        }

        pub fn participants(&mut self, participants: HashSet<SignerID>) -> &mut Self {
            self.participants = participants;
            self
        }

        pub fn master_index(&mut self, master_index: usize) -> &mut Self {
            self.master_index = master_index;
            self
        }

        pub fn block_height(&mut self, block_height: u64) -> &mut Self {
            self.block_height = block_height;
            self
        }
    }
}
