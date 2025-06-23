#![cfg_attr(not(feature = "std"), no_std)]

use ink::prelude::string::String;
use ink::storage::Mapping;

#[ink::contract]
mod orb_registry {
    #[ink(event)]
    pub struct OrbRegistered {
        #[ink(topic)]
        orb_id: AccountId,
        metadata: String,
    }

    #[ink(event)]
    pub struct OrbAttestation {
        #[ink(topic)]
        orb_id: AccountId,
        #[ink(topic)]
        user: AccountId,
    }

    #[ink(storage)]
    pub struct OrbRegistry {
        metadata_by_orb: Mapping<AccountId, String>,
        operator_by_orb: Mapping<AccountId, AccountId>, // Orb ID → Operator
    }

    impl OrbRegistry {
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                metadata_by_orb: Mapping::default(),
                operator_by_orb: Mapping::default(),
            }
        }

        /// Register a new Orb with metadata. The caller is set as the operator.
        #[ink(message)]
        pub fn register_orb(&mut self, orb_id: AccountId, metadata: String) {
            let caller = self.env().caller();
            self.metadata_by_orb.insert(orb_id, &metadata);
            self.operator_by_orb.insert(orb_id, &caller);
            self.env().emit_event(OrbRegistered { orb_id, metadata });
        }

        /// Emit an attestation event by orb and user.
        #[ink(message)]
        pub fn attestation(&self, orb_id: AccountId, user: AccountId) {
            self.env().emit_event(OrbAttestation { orb_id, user });
        }

        /// Get metadata associated with an Orb.
        #[ink(message)]
        pub fn get_metadata(&self, orb_id: AccountId) -> Option<String> {
            self.metadata_by_orb.get(orb_id)
        }

        /// Verify that a given operator owns the specified Orb.
        #[ink(message)]
        pub fn verify_ownership(&self, orb_id: AccountId, operator: AccountId) -> bool {
            match self.operator_by_orb.get(orb_id) {
                Some(registered_operator) => registered_operator == operator,
                None => false,
            }
        }

        /// Get the operator associated with a given Orb.
        #[ink(message)]
        pub fn get_operator(&self, orb_id: AccountId) -> Option<AccountId> {
            self.operator_by_orb.get(orb_id)
        }
    }
}
