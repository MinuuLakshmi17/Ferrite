use crate::bencode::{decode, encode, Value};
use anyhow::Result;
use rand::RngCore;
use std::{
    collections::BTreeMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{net::UdpSocket, time::timeout};

#[derive(Clone, Debug)]
pub struct DhtPeer {
    pub addr: SocketAddr,
}
#[derive(Clone)]
pub struct DhtClient {
    socket: std::sync::Arc<UdpSocket>,
    id: [u8; 20],
}
impl DhtClient {
    pub async fn bind(id: [u8; 20]) -> Result<Self> {
        let s = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket: std::sync::Arc::new(s),
            id,
        })
    }
    pub async fn get_peers(&self, info_hash: [u8; 20], bootstrap: &[SocketAddr]) -> Result<Vec<DhtPeer>> {
        let mut peers = Vec::new();
        for b in bootstrap {
            if let Ok(v) = self.query(b"get_peers", *b, info_hash).await {
                if let Some(p) = parse_values(&v) {
                    peers.extend(p);
                }
            }
        }
        peers.sort_by_key(|p| p.addr);
        peers.dedup_by_key(|p| p.addr);
        Ok(peers)
    }
    async fn query(&self, method: &[u8], addr: SocketAddr, info_hash: [u8; 20]) -> Result<Value> {
        let mut tid = [0u8; 2];
        rand::thread_rng().fill_bytes(&mut tid);
        let mut a = BTreeMap::new();
        a.insert(b"id".to_vec(), Value::Bytes(self.id.to_vec()));
        a.insert(b"info_hash".to_vec(), Value::Bytes(info_hash.to_vec()));
        let mut args = BTreeMap::new();
        args.insert(b"id".to_vec(), Value::Bytes(self.id.to_vec()));
        args.insert(b"info_hash".to_vec(), Value::Bytes(info_hash.to_vec()));
        let mut root = BTreeMap::new();
        root.insert(b"a".to_vec(), Value::Dict(args));
        root.insert(b"q".to_vec(), Value::Bytes(method.to_vec()));
        root.insert(b"t".to_vec(), Value::Bytes(tid.to_vec()));
        root.insert(b"y".to_vec(), Value::Bytes(b"q".to_vec()));
        let mut out = Vec::new();
        encode(&Value::Dict(root), &mut out);
        self.socket.send_to(&out, addr).await?;
        let mut buf = [0u8; 65535];
        let (n, _) = timeout(Duration::from_secs(4), self.socket.recv_from(&mut buf)).await??;
        let (v, _) = decode(&buf[..n])?;
        Ok(v)
    }
}
fn parse_values(v: &Value) -> Option<Vec<DhtPeer>> {
    let r = v.get(b"r").ok()?;
    let values = r.get(b"values").ok()?;
    let mut out = Vec::new();
    for x in values.list().ok()? {
        let b = x.as_bytes().ok()?;
        for c in b.as_chunks::<6>().0 {
            out.push(DhtPeer {
                addr: SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::new(c[0], c[1], c[2], c[3])),
                    u16::from_be_bytes([c[4], c[5]]),
                ),
            });
        }
    }
    Some(out)
}
pub fn default_bootstrap() -> Vec<SocketAddr> {
    ["67.215.231.242:6881", "82.221.103.244:6881"]
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bootstrap_nonempty() {
        assert!(!default_bootstrap().is_empty());
    }
}
