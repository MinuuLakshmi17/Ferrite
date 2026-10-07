use crate::{
    piece::{Bitfield, PieceManager},
    torrent::TorrentMeta,
};
use anyhow::{bail, Result};
use bytes::{BufMut, BytesMut};
use rand::RngCore;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
    time::timeout,
};
use tracing::debug;

const PSTR: &[u8] = b"BitTorrent protocol";
#[derive(Clone, Debug)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have(u32),
    Bitfield(Vec<u8>),
    Request { index: u32, begin: u32, length: u32 },
    Piece { index: u32, begin: u32, data: Vec<u8> },
    Cancel { index: u32, begin: u32, length: u32 },
    Port(u16),
    Unknown(u8, Vec<u8>),
}

pub struct Peer {
    stream: TcpStream,
    pub addr: SocketAddr,
    peer_choking: bool,
    bitfield: Option<Bitfield>,
}
impl Peer {
    pub async fn connect(addr: SocketAddr, meta: &TorrentMeta, peer_id: [u8; 20]) -> Result<Self> {
        let stream = timeout(Duration::from_secs(10), TcpStream::connect(addr)).await??;
        let mut p = Self {
            stream,
            addr,
            peer_choking: true,
            bitfield: None,
        };
        p.handshake(&meta.info_hash, peer_id).await?;
        Ok(p)
    }
    async fn handshake(&mut self, info_hash: &[u8; 20], peer_id: [u8; 20]) -> Result<()> {
        let mut out = Vec::with_capacity(68);
        out.push(PSTR.len() as u8);
        out.extend_from_slice(PSTR);
        out.extend_from_slice(&[0; 8]);
        out.extend_from_slice(info_hash);
        out.extend_from_slice(&peer_id);
        self.stream.write_all(&out).await?;
        let mut h = [0u8; 68];
        timeout(Duration::from_secs(10), self.stream.read_exact(&mut h)).await??;
        if h[0] != 19 || &h[1..20] != PSTR {
            bail!("invalid BitTorrent handshake")
        };
        if &h[28..48] != info_hash {
            bail!("peer info-hash mismatch")
        };
        Ok(())
    }
    pub async fn run(
        mut self,
        meta: TorrentMeta,
        manager: PieceManager,
        storage: std::sync::Arc<crate::storage::Storage>,
    ) -> Result<usize> {
        // Request-driven loop: whenever this peer is unchoked and holds a piece
        // we still need, ask for it immediately instead of waiting for inbound
        // traffic. We only block on `recv` when there is nothing to request.
        self.send(Message::Interested).await?;
        let mut downloaded = 0usize;
        loop {
            if manager.is_complete().await {
                break;
            }
            let want = if self.peer_choking {
                None
            } else {
                manager
                    .next_rarest()
                    .await
                    .filter(|&i| self.bitfield.as_ref().map(|b| b.has(i)).unwrap_or(true))
            };
            match want {
                Some(index) => {
                    if !manager.claim(index).await {
                        continue; // lost a race with another peer task; re-evaluate
                    }
                    match self.download_piece(&meta, &manager, &storage, index).await {
                        Ok(buf) => {
                            downloaded += buf.len();
                            manager.mark_complete(index).await;
                        }
                        Err(e) => {
                            debug!(%index, error=%e, "piece download failed, released for retry");
                            manager.release(index).await;
                        }
                    }
                }
                None => {
                    // Nothing to request right now: wait for choke/unchoke/have traffic.
                    let msg = match timeout(Duration::from_secs(45), self.recv()).await {
                        Ok(x) => x?,
                        Err(_) => break,
                    };
                    self.on_message(msg, &manager).await;
                }
            }
        }
        Ok(downloaded)
    }

    /// Update choke state and piece availability from one inbound message.
    async fn on_message(&mut self, msg: Message, manager: &PieceManager) {
        match msg {
            Message::Choke => self.peer_choking = true,
            Message::Unchoke => self.peer_choking = false,
            Message::Bitfield(b) => {
                self.bitfield = Some(Bitfield::new(b));
                if let Some(ref bf) = self.bitfield {
                    manager.register_bitfield(bf).await;
                }
            }
            Message::Have(i) => manager.register_have(i as usize).await,
            _ => {}
        }
    }

    /// Request every 16 KiB block of one piece and verify its SHA-1.
    /// Returns the verified bytes; any failure releases the claim in `run`.
    async fn download_piece(
        &mut self,
        meta: &TorrentMeta,
        manager: &PieceManager,
        storage: &std::sync::Arc<crate::storage::Storage>,
        index: usize,
    ) -> Result<Vec<u8>> {
        let block = 16 * 1024;
        let total = meta.total_length();
        let expected = crate::piece::expected_piece_size(total, meta.info.piece_length, index, manager.count());
        let mut buf = vec![0u8; expected];
        for block_no in 0..expected.div_ceil(block) {
            let begin = block_no * block;
            let len = (expected - begin).min(block);
            self.send(Message::Request {
                index: index as u32,
                begin: begin as u32,
                length: len as u32,
            })
            .await?;
            loop {
                let incoming = timeout(Duration::from_secs(30), self.recv()).await??;
                match incoming {
                    Message::Piece {
                        index: i,
                        begin: b,
                        data,
                    } if i as usize == index && b as usize == begin => {
                        if data.len() != len {
                            bail!("short block: got {}, want {len}", data.len());
                        }
                        buf[begin..begin + len].copy_from_slice(&data);
                        break;
                    }
                    Message::Choke => {
                        self.peer_choking = true;
                        bail!("choked mid-piece");
                    }
                    Message::Have(i) => manager.register_have(i as usize).await,
                    Message::Bitfield(b) => {
                        self.bitfield = Some(Bitfield::new(b));
                    }
                    _ => {}
                }
            }
        }
        if !manager.verify(index, &buf).await.unwrap_or(false) {
            bail!("piece {index} failed SHA-1 verification");
        }
        storage.write_piece(index as u64 * meta.info.piece_length, &buf)?;
        Ok(buf)
    }

    async fn send(&mut self, m: Message) -> Result<()> {
        let mut p = BytesMut::new();
        match m {
            Message::KeepAlive => {
                p.put_u32(0);
            }
            Message::Choke => Self::frame(0, &[], &mut p),
            Message::Unchoke => Self::frame(1, &[], &mut p),
            Message::Interested => Self::frame(2, &[], &mut p),
            Message::NotInterested => Self::frame(3, &[], &mut p),
            Message::Have(i) => {
                let mut b = BytesMut::new();
                b.put_u32(i);
                Self::frame(4, &b, &mut p)
            }
            Message::Request { index, begin, length } => {
                let mut b = BytesMut::new();
                b.put_u32(index);
                b.put_u32(begin);
                b.put_u32(length);
                Self::frame(6, &b, &mut p)
            }
            Message::Cancel { index, begin, length } => {
                let mut b = BytesMut::new();
                b.put_u32(index);
                b.put_u32(begin);
                b.put_u32(length);
                Self::frame(8, &b, &mut p)
            }
            Message::Port(port) => {
                let mut b = BytesMut::new();
                b.put_u16(port);
                Self::frame(9, &b, &mut p)
            }
            Message::Bitfield(b) => Self::frame(5, &b, &mut p),
            Message::Piece { index, begin, data } => {
                let mut b = BytesMut::new();
                b.put_u32(index);
                b.put_u32(begin);
                b.extend_from_slice(&data);
                Self::frame(7, &b, &mut p)
            }
            Message::Unknown(_, _) => return Ok(()),
        };
        self.stream.write_all(&p).await?;
        Ok(())
    }
    fn frame(id: u8, payload: &[u8], out: &mut BytesMut) {
        out.put_u32((payload.len() + 1) as u32);
        out.put_u8(id);
        out.extend_from_slice(payload)
    }
    async fn recv(&mut self) -> Result<Message> {
        let len = self.stream.read_u32().await?;
        if len == 0 {
            return Ok(Message::KeepAlive);
        }
        if len > 2_000_000 {
            bail!("peer message too large: {len}")
        }
        let mut b = vec![0u8; len as usize];
        self.stream.read_exact(&mut b).await?;
        if b.is_empty() {
            bail!("empty peer message")
        }
        let id = b[0];
        let p = &b[1..];
        Ok(match id {
            0 => Message::Choke,
            1 => Message::Unchoke,
            2 => Message::Interested,
            3 => Message::NotInterested,
            4 => Message::Have(read_u32(p)?),
            5 => Message::Bitfield(p.to_vec()),
            6 => Message::Request {
                index: read_u32_at(p, 0)?,
                begin: read_u32_at(p, 4)?,
                length: read_u32_at(p, 8)?,
            },
            7 => Message::Piece {
                index: read_u32_at(p, 0)?,
                begin: read_u32_at(p, 4)?,
                data: p
                    .get(8..)
                    .ok_or_else(|| anyhow::anyhow!("short piece message"))?
                    .to_vec(),
            },
            8 => Message::Cancel {
                index: read_u32_at(p, 0)?,
                begin: read_u32_at(p, 4)?,
                length: read_u32_at(p, 8)?,
            },
            9 => Message::Port(u16::from_be_bytes([
                *p.first().ok_or_else(|| anyhow::anyhow!("short port message"))?,
                *p.get(1).ok_or_else(|| anyhow::anyhow!("short port message"))?,
            ])),
            x => Message::Unknown(x, p.to_vec()),
        })
    }
}
fn read_u32(p: &[u8]) -> Result<u32> {
    read_u32_at(p, 0)
}
fn read_u32_at(p: &[u8], o: usize) -> Result<u32> {
    let x = p.get(o..o + 4).ok_or_else(|| anyhow::anyhow!("short message"))?;
    Ok(u32::from_be_bytes([x[0], x[1], x[2], x[3]]))
}
pub fn random_peer_id() -> [u8; 20] {
    let mut x = [0u8; 20];
    rand::thread_rng().fill_bytes(&mut x);
    x[0] = b'-';
    x[1] = b'F';
    x[2] = b'R';
    x[3] = b'0';
    x
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn frame_request() {
        let mut b = BytesMut::new();
        let mut p = BytesMut::new();
        p.put_u32(1);
        p.put_u32(2);
        p.put_u32(3);
        Peer::frame(6, &p, &mut b);
        assert_eq!(&b[..4], &13u32.to_be_bytes());
        assert_eq!(b[4], 6);
    }
}
