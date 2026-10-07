pub mod bencode;
pub mod dht;
pub mod peer;
pub mod piece;
pub mod storage;
pub mod torrent;
pub mod tracker;

pub const CLIENT_ID: &[u8; 20] = b"-FR0001-ferrite00001";

#[cfg(test)]
mod tests {
    use super::CLIENT_ID;
    #[test]
    fn client_id_is_20_bytes() {
        assert_eq!(CLIENT_ID.len(), 20);
    }
}
