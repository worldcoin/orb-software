
# 📦 PR: On-Chain Orb Registry — Rust //Implementation

## 🎯 Purpose

This PR introduces a minimal `ink!` smart contract called `OrbRegistry` to enable on-chain representation and verification of Orb devices for increased transparency within the Worldcoin ecosystem.

## 🧭 Context & Motivation

Currently, there's no verifiable, on-chain method to track or validate active Orb devices. This limits:

* Transparency into which Orbs are operational.
* Independent auditability of attestation activity.
* On-chain integrations and composability with third-party tools.

This contract allows each Orb to be uniquely registered and for each attestation to be publicly logged.

## 🛠️ What This PR Includes

* ✅ `OrbRegistry` contract written in Rust using `ink!`
* ✅ Support for registering Orbs with a unique `AccountId` and metadata
* ✅ Emitting `OrbRegistered` and `OrbAttestation` events
* ✅ Public function to fetch Orb metadata by `orb_id`
* ✅ Stateless attestation method for recording proof interactions

## 🧪 Sample Contract Logic (ink!)

```rust
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
}

impl OrbRegistry {
    #[ink(constructor)]
    pub fn new() -> Self {
        Self {
            metadata_by_orb: Mapping::default(),
        }
    }

    #[ink(message)]
    pub fn register_orb(&mut self, orb_id: AccountId, metadata: String) {
        self.metadata_by_orb.insert(orb_id, &metadata);
        self.env().emit_event(OrbRegistered { orb_id, metadata });
    }

    #[ink(message)]
    pub fn attestation(&self, orb_id: AccountId, user: AccountId) {
        self.env().emit_event(OrbAttestation { orb_id, user });
    }

    #[ink(message)]
    pub fn get_metadata(&self, orb_id: AccountId) -> Option<String> {
        self.metadata_by_orb.get(orb_id)
    }
}
```

## ✅ Benefits

* **Transparency**: Public attestation logs and registry
* **Security**: On-chain provenance of Orb actions
* **Extendability**: Ready for integration with relayers, dashboards, or future governance hooks

## 📌 Follow-ups (for future PRs)

* Add access controls (e.g., only trusted relayers or operators can register)
* Gas optimizations for large-scale deployments
* UI or explorer integration for public auditability

