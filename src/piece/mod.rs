use anyhow::Result;
use sha1::{Digest, Sha1};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Clone)]
pub struct PieceManager {
    hashes: Arc<Vec<[u8; 20]>>,
    states: Arc<Mutex<Vec<State>>>,
}
#[derive(Clone, Debug)]
struct State {
    complete: bool,
    in_flight: bool,
    availability: usize,
}
impl PieceManager {
    pub fn new(hashes: Vec<[u8; 20]>) -> Self {
        let n = hashes.len();
        Self {
            hashes: Arc::new(hashes),
            states: Arc::new(Mutex::new(
                (0..n)
                    .map(|_| State {
                        complete: false,
                        in_flight: false,
                        availability: 0,
                    })
                    .collect(),
            )),
        }
    }
    pub async fn register_bitfield(&self, bf: &Bitfield) {
        let mut s = self.states.lock().await;
        for i in 0..self.hashes.len() {
            if bf.has(i) {
                s[i].availability += 1;
            }
        }
    }
    pub async fn register_have(&self, i: usize) {
        if let Some(s) = self.states.lock().await.get_mut(i) {
            s.availability += 1;
        }
    }
    pub async fn next_rarest(&self) -> Option<usize> {
        let s = self.states.lock().await;
        let open = |x: &State| !x.complete && !x.in_flight;
        s.iter()
            .enumerate()
            .filter(|(_, x)| open(x) && x.availability > 0)
            .min_by_key(|(_, x)| x.availability)
            .map(|(i, _)| i)
            .or_else(|| s.iter().position(open))
    }
    pub async fn claim(&self, i: usize) -> bool {
        match self.states.lock().await.get_mut(i) {
            Some(s) if !s.complete && !s.in_flight => {
                s.in_flight = true;
                true
            }
            _ => false,
        }
    }
    pub async fn release(&self, i: usize) {
        if let Some(s) = self.states.lock().await.get_mut(i) {
            s.in_flight = false;
        }
    }
    pub async fn mark_complete(&self, i: usize) {
        if let Some(s) = self.states.lock().await.get_mut(i) {
            s.complete = true;
            s.in_flight = false;
        }
    }
    pub async fn is_complete(&self) -> bool {
        self.states.lock().await.iter().all(|x| x.complete)
    }
    pub fn hash(&self, i: usize) -> Option<[u8; 20]> {
        self.hashes.get(i).copied()
    }
    pub async fn verify(&self, i: usize, data: &[u8]) -> Result<bool> {
        let expected = self.hash(i).ok_or_else(|| anyhow::anyhow!("invalid piece"))?;
        Ok(Sha1::digest(data).as_slice() == expected)
    }
    pub fn count(&self) -> usize {
        self.hashes.len()
    }
}
#[derive(Clone, Debug)]
pub struct Bitfield {
    bits: Vec<u8>,
}
impl Bitfield {
    pub fn new(bits: Vec<u8>) -> Self {
        Self { bits }
    }
    pub fn has(&self, i: usize) -> bool {
        self.bits
            .get(i / 8)
            .map(|b| b & (0x80 >> (i % 8)) != 0)
            .unwrap_or(false)
    }
}
pub fn expected_piece_size(total: u64, piece_len: u64, index: usize, count: usize) -> usize {
    if index + 1 < count {
        piece_len as usize
    } else {
        (total - piece_len * ((count.saturating_sub(1)) as u64)) as usize
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn rarest() {
        let mut a = [0; 20];
        a[0] = 1;
        let m = PieceManager::new(vec![a, [0; 20]]);
        m.register_have(0).await;
        assert_eq!(m.next_rarest().await, Some(0));
        m.mark_complete(0).await;
        assert_eq!(m.next_rarest().await, Some(1));
    }
    #[test]
    fn size() {
        assert_eq!(expected_piece_size(20, 8, 2, 3), 4);
    }
}
