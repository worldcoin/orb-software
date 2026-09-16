[iPCP Unified Pairing Protocol](https://app.notion.com/p/worldcoin/IPCP-Unified-Pairing-Protocol-Hardening-Proposal-3ab8614bdf8c807da2faf0f5d6f185db)

`PairingKey` owns a private key and exposes its corresponding `pairing_public_key`.
Its three public operations are `new`, `encrypt`, and `decrypt`. The sender only
needs the recipient's public key to encrypt; the recipient keeps the private key
inside its `PairingKey` to decrypt. Treat the public key field as read-only so it
continues to match the private key.

```rust
use orb_ipcp_hpke::PairingKey;

let recipient = PairingKey::new()?;
let info = b"worldcoin/ipcp/hpke/v1";
let aad = b"";
let payload = PairingKey::encrypt(
    &recipient.pairing_public_key,
    b"image bytes".to_vec(),
    info,
    aad,
)?;
let plaintext = recipient.decrypt(&payload, info, aad)?;
```

The caller supplies matching protocol context (`info`) and authenticated metadata
(`aad`) for encryption and decryption. Operations are synchronous; callers choose
how to schedule them.
