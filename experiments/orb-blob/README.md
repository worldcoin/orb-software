# orb-blob Proof-of-Concept

**NOTE: This is a proof of concept built during the hackathon in July. It is not
intended to be deployed on orbs in its current form.**

![video demo showcasing orb-blob and explaining it](./demo.mp4)

# What is it?

- Files can be transferred directly from device to device (peer to peer), without needing any centralized backend services.
- It is fully decentralized and permissionless. Built using the latest in peer to peer tech.
- Anyone can run this software, allowing users not just to own the orb, but to use it without relying on us to provide backend services for it.
- It can be used for Over the air Updates, saving tons of bandwidth by having orbs talk to each other, instead of our backend.
- Can help facilitate direct transfer of personal-custody package to users phone, improving signup UX in bad networks - especially when paired with Wifi Direct or local networks.

# Tech Stack

All of the following is open source, selfhostable, and lends itself to decentralized/unpermissioned/trustless infrastructure.

- [iroh](https://www.iroh.computer/) - [QUIC](https://datatracker.ietf.org/doc/html/rfc9000) (what HTTP3 uses), but peer to peer.
    - nodes dial each other by public key instead of by IP Address.
    - QUIC is like TCP + UDP but multiplexed all into one amazing API.
    - Everything uses mTLS
    - Seamless migration of endpoints (switching from 4G to WIFI) without breaking connection or TLS.
    - Handles hole punching and NAT traversal, with support for falling back to trustless relays if hole punching fails.
- [Public Key Addressable Resource Records (PKARR)](https://github.com/pubky/pkarr) - what iroh uses for discovery of nodes
    - (optional): Supports using Bittorrent’s mainline [Distributed Hash Table](https://www.youtube.com/watch?v=1QdKhNpsj8M&pp=ygUWZGlzdHJpYnV0ZWQgaGFzaCB0YWJsZdIHCQnHCQGHKiGM7w%3D%3D) (Kademlia DHT).
    - (optional): Supports using [Multicast DNS](https://datatracker.ietf.org/doc/html/rfc6762) for local use cases
- [iroh-gossip](https://docs.rs/iroh-gossip/0.91.0/iroh_gossip/index.html) - a gossip protocol built on iroh, that we use to implement file discovery.
    - It uses [HyParView](https://asc.di.fct.unl.pt/~jleitao/pdf/dsn07-leitao.pdf) algorithm to form an overlay network, and [PlumTree](https://asc.di.fct.unl.pt/~jleitao/pdf/srds07-leitao.pdf) algorithm for gossip. These details are abstracted away.
- [iroh-blobs](https://docs.rs/iroh-blobs/0.93.0/iroh_blobs/index.html) - a peer to peer file transfer protocol built on iroh.
    - All files are addressed by [blake3 hash.](https://www.infoq.com/news/2020/01/blake3-fast-crypto-hash/)
    - supports efficient verified file streaming and range requests.
    - supports multiple providers for files, increasing download speeds by seeding from multiple nodes at the same time.
- [axum](https://docs.rs/axum/latest/axum/) - rust http framework
- [sqlx](https://docs.rs/sqlx/latest/sqlx/) - straightforward client for SQLite database

# How it Works

We get almost all of the decentralization, encryption, and peer to peer goodness just by using iroh and its ecosystem (iroh-blobs, iroh-gossip) off the shelf.

There are really only two things we had to build:

### Discovering Nodes that pin files

before we can retrieve files, we have to find *which* nodes have the file in the first place. To solve this, we implemented a simple [iroh-gossip based discovery system](https://github.com/worldcoin/orb-software/blob/1e917cc5b1472091baf013ad984c8c0c5a181b09/orb-blob/p2p/src/lib.rs#L40-L55), where all nodes join a single iroh gossip topic, and then anyone who pins the file occasionally broadcasts their NodeId (public key). This allows anyone listening on the gossip topic to locate those nodes.

One caveat here is that joining a gossip network still requires joining some initial set of nodes from which the full gossip swarm can bootstrap from. We implemented this via a simple “well known nodes” mechanism, where we can retrieve a list of node pubkeys from a fixed set of HTTP endpoints. One can imagine that Tools for Humanity, any other orb manufacturers, and/or the AMPC cluster would publish lists of the bootstrap nodes they operate, to facilitate peers to find each other. Its important to note that bootstrap nodes are just regular nodes in the swarm, are permissionless, and can be operated by third parties. Its just a way to join the swarm - any node in the swarm will do.

### Managing File Pinning and Transfer

File management is implemented using [a REST api in rust’s axum framework](https://github.com/worldcoin/orb-software/blob/1e917cc5b1472091baf013ad984c8c0c5a181b09/orb-blob/src/program.rs#L66-L73), with sqlite as the way we track files. It ostensibly could be done without any REST api at all, we just went with axum as a convenient way to daemonize the service. This service is intended for purely local use - i.e. localhost. It doesn’t need to be exposed to the public, because its just a management api.

[The actual transfer of files](https://github.com/worldcoin/orb-software/blob/1e917cc5b1472091baf013ad984c8c0c5a181b09/orb-blob/src/handlers/download.rs#L23-L115) is performed by leveraging the aforementioned gossip based file discovery, as well as the iroh-blobs crate directly. Transfer of the files happens entirely peer to peer, and doesn't require port forwarding on orbs or retrieving from s3. The files can go straight from device to device. If hole punching fails, the quic/iroh connection is proxied through a trustless iroh relay, ensuring it always works. Even better, iroh is smart enough to migrate to direct local connection when possible - if you dont have WAN but have LAN, it will still work.
