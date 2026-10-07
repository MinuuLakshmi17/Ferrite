# Ferrite

[![Rust](https://img.shields.io/badge/Rust-1.99-CE422B?logo=rust)](https://www.rust-lang.org)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Tests](https://img.shields.io/badge/tests-passing-brightgreen)](#build)
[![E2E](https://img.shields.io/badge/e2e-byte--identical%20download-brightgreen)](#build)

**Ferrite** is a from-scratch BitTorrent client written in Rust, designed as a systems/distributed-systems portfolio project rather than a wrapper around an existing torrent library.

> Educational software. Only use it with content you are legally allowed to download and share.

## What is implemented

### Core protocol
- Bencode decoder/encoder with recursive dictionaries/lists and validation
- `.torrent` metadata parsing
- Exact bencoded `info` dictionary hashing for the 20-byte SHA-1 info-hash
- HTTP tracker announce with compact peer-list decoding
- BitTorrent peer handshake
- Peer wire messages: keep-alive, choke, unchoke, interested, not-interested, have, bitfield, request, piece, cancel, port
- Concurrent TCP peer sessions using Tokio
- Piece verification using SHA-1
- 16 KiB block requests
- Multi-file torrent path mapping
- Rarest-first selection based on observed peer availability
- Peer timeouts and failure isolation

### Distributed-systems extension
- UDP DHT client
- Kademlia-style `get_peers` queries
- Compact IPv4 peer decoding
- Bootstrap nodes

### Engineering
- Modular crate architecture
- Async I/O
- Structured logging with `tracing`
- CLI built with `clap`
- Unit tests for Bencode, torrent metadata, piece management, and protocol framing
- End-to-end integration test (`tests/e2e`): builds a torrent, serves a fake
  HTTP tracker and peer swarm, downloads, and verifies the output byte-for-byte
- No unsafe Rust

## Deliberate scope boundaries

This version intentionally does **not** claim support for every BitTorrent BEP. In particular:

- HTTPS tracker certificates are handled by Reqwest/Rustls, but tracker behavior is HTTP(S) announce only.
- Full metadata exchange for magnet links (BEP 9) is not implemented.
- DHT peer discovery is implemented as a client query path; full routing-table maintenance, token validation, iterative lookup, and peer announcement are future extensions.
- Extension protocol/message-stream encryption and web seeds are omitted.
- IPv6 compact peer/DHT support is omitted from the initial implementation.

Those boundaries are explicit so the project can be extended without pretending a partial implementation is complete.

## Build

```bash
cargo fmt --all -- --check
cargo check --all-targets
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
# End-to-end: fake tracker + 3-peer swarm, byte-identical download
./tests/e2e/run.sh
```

## Usage

Inspect a torrent:

```bash
cargo run -- inspect ./ubuntu.torrent
```

Download using tracker-discovered peers:

```bash
cargo run --release -- download ./ubuntu.torrent --output ./downloads --peers 8
```

Query DHT for peers for a known info-hash:

```bash
cargo run --release -- dht <40-hex-character-info-hash>
```

## Architecture

```text
                  +----------------+
                  |      CLI       |
                  +-------+--------+
                          |
              +-----------+-----------+
              |                       |
        +-----v------+          +-----v------+
        |   Torrent  |          |     DHT    |
        |  Metadata  |          | UDP/Kademlia|
        +-----+------+          +-----+------+
              |                       |
        +-----v------+          +-----v------+
        |  Tracker   |          |   Peers    |
        | HTTP(S)    |          | discovery  |
        +-----+------+          +------------+
              |
       +------v-------+
       |  PieceManager |
       | rarest-first  |
       +------+--------+
              |
       +------v-------+
       | Peer Sessions |
       | Tokio/TCP     |
       +------+--------+
              |
       +------v-------+
       |   Storage     |
       | files/pieces  |
       +---------------+
```

## Suggested next-level extensions

For a research-heavy version, the next additions should be:

1. Full iterative Kademlia routing table with k-buckets and XOR-distance ordering.
2. BEP 9 metadata exchange for magnet links.
3. Persistent resume state and bitfield-on-disk.
4. Endgame mode and duplicate block requests.
5. Upload seeding and tit-for-tat choking/unchoking.
6. Tracker announce lifecycle (`started`, periodic, `completed`, `stopped`).
7. IPv6 and UDP tracker protocol.
8. Prometheus-style metrics export and benchmark harness.
9. Property-based tests for the Bencode parser.
10. Integration tests using a deterministic local tracker and local peer swarm.

## Resume bullet

> Built **Ferrite**, a BitTorrent client in Rust implementing Bencode parsing, tracker-based peer discovery, the BitTorrent peer wire protocol, concurrent piece downloads with rarest-first selection, SHA-1 piece verification, and UDP DHT peer discovery.

## License

MIT.
