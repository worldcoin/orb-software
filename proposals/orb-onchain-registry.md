# Proposal: On-Chain Orb Registry for Transparency

## 🎯 Summary

Introduce a system where each Orb device is uniquely represented on-chain, enabling real-time verifiability of active Orb devices on WorldChain or any EVM-compatible environment.

## 📌 Motivation

Currently, there is no direct on-chain method to verify the number or status of active Orb devices. This limits transparency and auditability, which are core to the Worldcoin mission.

## 🛠️ Implementation Proposal

- Each Orb is assigned a unique identifier (`orbId`)
- A new smart contract `OrbRegistry.sol` is deployed to store and emit `OrbRegistered` and `OrbAttestation` events
- Orb operator software emits on-chain logs during each attestation
- The backend emits these logs via a relayer or L2-compatible client

## 🔐 Contract Sample (Solidity)

```solidity
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.17;

contract OrbRegistry {
    event OrbRegistered(address indexed orbId, string metadata);
    event OrbAttestation(address indexed orbId, address user);

    function registerOrb(address orbId, string calldata metadata) external {
        emit OrbRegistered(orbId, metadata);
    }

    function attestation(address orbId, address user) external {
        emit OrbAttestation(orbId, user);
    }
}
