use crate::{
    bencode::{decode, Value},
    torrent::TorrentMeta,
};
use anyhow::{bail, Context, Result};
use reqwest::Client;

#[derive(Clone, Debug)]
pub struct PeerAddr {
    pub ip: String,
    pub port: u16,
}
#[derive(Clone, Debug)]
pub struct TrackerResponse {
    pub interval: u64,
    pub peers: Vec<PeerAddr>,
}

pub async fn announce(
    meta: &TorrentMeta,
    peer_id: [u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
) -> Result<TrackerResponse> {
    let client = Client::builder().user_agent("ferrite/0.1").build()?;
    let mut last = None;
    for url in &meta.announce {
        let req = AnnounceRequest {
            client: &client,
            url,
            meta,
            peer_id,
            port,
            uploaded,
            downloaded,
            left,
        };
        match announce_one(req).await {
            Ok(x) => return Ok(x),
            Err(e) => {
                last = Some(e);
            }
        }
    }
    Err(last.unwrap_or_else(|| anyhow::anyhow!("torrent has no tracker URLs")))
}

/// Parameters for a single tracker announce; bundled so `announce_one`
/// does not need eight positional arguments.
struct AnnounceRequest<'a> {
    client: &'a Client,
    url: &'a str,
    meta: &'a TorrentMeta,
    peer_id: [u8; 20],
    port: u16,
    uploaded: u64,
    downloaded: u64,
    left: u64,
}
async fn announce_one(req: AnnounceRequest<'_>) -> Result<TrackerResponse> {
    let AnnounceRequest {
        client,
        url,
        meta,
        peer_id,
        port,
        uploaded,
        downloaded,
        left,
    } = req;
    let sep = if url.contains('?') { '&' } else { '?' };
    let ih = percent(&meta.info_hash);
    let pid = percent(&peer_id);
    let u=format!("{url}{sep}info_hash={ih}&peer_id={pid}&port={port}&uploaded={uploaded}&downloaded={downloaded}&left={left}&compact=1&event=started");
    let bytes = client.get(u).send().await?.error_for_status()?.bytes().await?;
    let (v, _) = decode(&bytes)?;
    let d = v.dict()?;
    if let Some(f) = d.get(b"failure reason".as_slice()) {
        bail!("tracker failure: {}", f.as_str().unwrap_or("unknown"))
    };
    let interval = d
        .get(b"interval".as_slice())
        .and_then(|x| x.as_int().ok())
        .unwrap_or(1800) as u64;
    let peers = d.get(b"peers".as_slice()).context("tracker response missing peers")?;
    let mut out = Vec::new();
    match peers {
        Value::Bytes(p) => {
            for c in p.as_chunks::<6>().0 {
                out.push(PeerAddr {
                    ip: format!("{}.{}.{}.{}", c[0], c[1], c[2], c[3]),
                    port: u16::from_be_bytes([c[4], c[5]]),
                });
            }
        }
        Value::List(xs) => {
            for p in xs {
                let pd = p.dict()?;
                let ip = pd
                    .get(b"ip".as_slice())
                    .ok_or_else(|| anyhow::anyhow!("missing key ip"))?
                    .as_str()?
                    .to_string();
                let port = pd
                    .get(b"port".as_slice())
                    .ok_or_else(|| anyhow::anyhow!("missing key port"))?
                    .as_int()? as u16;
                out.push(PeerAddr { ip, port });
            }
        }
        _ => bail!("unsupported peers encoding"),
    };
    Ok(TrackerResponse { interval, peers: out })
}
fn percent(b: &[u8]) -> String {
    b.iter().map(|x| format!("%{x:02X}")).collect()
}
