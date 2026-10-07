use ferrite::bencode::{decode, encode, Value};
use std::collections::BTreeMap;

#[test]
fn nested_values_round_trip() {
    let mut d = BTreeMap::new();
    d.insert(b"integer".to_vec(), Value::Int(-17));
    d.insert(b"bytes".to_vec(), Value::Bytes(vec![0, 1, 255]));
    d.insert(
        b"list".to_vec(),
        Value::List(vec![Value::Int(1), Value::Bytes(b"x".to_vec())]),
    );
    let original = Value::Dict(d);
    let mut bytes = Vec::new();
    encode(&original, &mut bytes);
    let (decoded, used) = decode(&bytes).expect("valid bencode");
    assert_eq!(used, bytes.len());
    assert_eq!(decoded, original);
}
