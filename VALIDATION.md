# Validation status

The crate was reviewed, repaired, and executed on a machine with Rust
1.99.0 installed. What was checked:

- `cargo fmt --all -- --check` — clean
- `cargo check --all-targets` — clean
- `cargo clippy --all-targets -- -D warnings` — clean
- `cargo test` — 9 tests pass (bencode round-trips, torrent parsing,
  piece framing, rarest-first selection, DHT bootstrap, client-id)
- `tests/e2e/run.sh` — full download against a fake HTTP tracker and a
  fake 3-peer swarm: 200 KiB file, 7 pieces, SHA-1 verified per piece,
  output byte-identical to the source (`complete=true`)

Bugs found and fixed during validation:

1. The tree did not compile (23 errors): `BTreeMap<Vec<u8>, _>::get`
   called with `&[u8; N]` instead of `&[u8]`, and `?` applied to
   `Option` values.
2. The peer download loop was recv-driven: after finishing a piece it
   blocked up to 45s waiting for inbound traffic instead of requesting
   the next piece, so quiet peers stalled every piece. Rewrote as a
   request-driven loop with per-piece claims (`claim`/`release`) so
   concurrent peers never download the same piece twice.
3. `Display` for bencode byte strings emitted `{:?}` (with quotes),
   producing invalid bencode; fixed.
4. DHT queried every bootstrap node twice and discarded the first
   round; merged into a single round.
5. A non-resolvable bootstrap hostname (`router.bittorrent.com:6881`)
   could never parse as `SocketAddr`; removed.
6. Downloaded-byte accounting over-subtracted on piece failure; now
   counts only verified pieces.
7. Identity: crate authors and LICENSE named the scaffold source;
   corrected to the repository owner.

Not yet covered: real-internet download against public trackers (the
sandboxed network path was not exercised beyond the fake swarm), and
DHT iterative lookup, which remains a documented future extension.
