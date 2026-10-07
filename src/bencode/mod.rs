use anyhow::{anyhow, bail, Result};
use std::{collections::BTreeMap, fmt};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Value {
    Int(i64),
    Bytes(Vec<u8>),
    List(Vec<Value>),
    Dict(BTreeMap<Vec<u8>, Value>),
}

impl Value {
    pub fn as_bytes(&self) -> Result<&[u8]> {
        match self {
            Self::Bytes(v) => Ok(v),
            _ => bail!("expected bytes"),
        }
    }
    pub fn as_str(&self) -> Result<&str> {
        Ok(std::str::from_utf8(self.as_bytes()?)?)
    }
    pub fn as_int(&self) -> Result<i64> {
        match self {
            Self::Int(v) => Ok(*v),
            _ => bail!("expected integer"),
        }
    }
    pub fn dict(&self) -> Result<&BTreeMap<Vec<u8>, Value>> {
        match self {
            Self::Dict(v) => Ok(v),
            _ => bail!("expected dictionary"),
        }
    }
    pub fn list(&self) -> Result<&[Value]> {
        match self {
            Self::List(v) => Ok(v),
            _ => bail!("expected list"),
        }
    }
    pub fn get(&self, key: &[u8]) -> Result<&Value> {
        self.dict()?
            .get(key)
            .ok_or_else(|| anyhow!("missing key {:?}", String::from_utf8_lossy(key)))
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Int(v) => write!(f, "i{v}e"),
            Self::Bytes(v) => write!(f, "{}:{}", v.len(), String::from_utf8_lossy(v)),
            Self::List(v) => {
                write!(f, "l")?;
                for x in v {
                    write!(f, "{x}")?;
                }
                write!(f, "e")
            }
            Self::Dict(v) => {
                write!(f, "d")?;
                for (k, x) in v {
                    write!(f, "{}:{}", k.len(), String::from_utf8_lossy(k))?;
                    write!(f, "{x}")?;
                }
                write!(f, "e")
            }
        }
    }
}

pub fn decode(input: &[u8]) -> Result<(Value, usize)> {
    let mut p = Parser { input, pos: 0 };
    let v = p.value()?;
    Ok((v, p.pos))
}

struct Parser<'a> {
    input: &'a [u8],
    pos: usize,
}
impl<'a> Parser<'a> {
    fn value(&mut self) -> Result<Value> {
        match self
            .input
            .get(self.pos)
            .copied()
            .ok_or_else(|| anyhow!("unexpected EOF"))?
        {
            b'i' => self.int(),
            b'l' => self.list(),
            b'd' => self.dict(),
            b'0'..=b'9' => self.bytes(),
            _ => bail!("invalid bencode marker at {}", self.pos),
        }
    }
    fn int(&mut self) -> Result<Value> {
        self.pos += 1;
        let start = self.pos;
        while let Some(&b) = self.input.get(self.pos) {
            if b == b'e' {
                let s = std::str::from_utf8(&self.input[start..self.pos])?;
                if s.is_empty() || (s.len() > 1 && s.starts_with('0')) || (s.len() > 2 && s.starts_with("-0")) {
                    bail!("invalid integer")
                };
                let n = s.parse::<i64>()?;
                self.pos += 1;
                return Ok(Value::Int(n));
            }
            self.pos += 1;
        }
        bail!("unterminated integer")
    }
    fn bytes(&mut self) -> Result<Value> {
        let start = self.pos;
        while let Some(&b) = self.input.get(self.pos) {
            if b == b':' {
                let n = std::str::from_utf8(&self.input[start..self.pos])?.parse::<usize>()?;
                self.pos += 1;
                let end = self.pos.checked_add(n).ok_or_else(|| anyhow!("length overflow"))?;
                let out = self
                    .input
                    .get(self.pos..end)
                    .ok_or_else(|| anyhow!("byte string exceeds input"))?
                    .to_vec();
                self.pos = end;
                return Ok(Value::Bytes(out));
            }
            if !b.is_ascii_digit() {
                bail!("invalid byte string length")
            };
            self.pos += 1;
        }
        bail!("unterminated byte string length")
    }
    fn list(&mut self) -> Result<Value> {
        self.pos += 1;
        let mut v = Vec::new();
        while self.input.get(self.pos) != Some(&b'e') {
            v.push(self.value()?);
        }
        self.pos += 1;
        Ok(Value::List(v))
    }
    fn dict(&mut self) -> Result<Value> {
        self.pos += 1;
        let mut d = BTreeMap::new();
        while self.input.get(self.pos) != Some(&b'e') {
            let k = match self.bytes()? {
                Value::Bytes(v) => v,
                _ => unreachable!(),
            };
            let val = self.value()?;
            if d.insert(k, val).is_some() {
                bail!("duplicate dictionary key")
            };
        }
        self.pos += 1;
        Ok(Value::Dict(d))
    }
}

pub fn encode(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Int(n) => {
            out.extend_from_slice(b"i");
            out.extend_from_slice(n.to_string().as_bytes());
            out.push(b'e')
        }
        Value::Bytes(b) => {
            out.extend_from_slice(b.len().to_string().as_bytes());
            out.push(b':');
            out.extend_from_slice(b)
        }
        Value::List(xs) => {
            out.push(b'l');
            for x in xs {
                encode(x, out)
            }
            out.push(b'e')
        }
        Value::Dict(d) => {
            out.push(b'd');
            for (k, v) in d {
                encode(&Value::Bytes(k.clone()), out);
                encode(v, out)
            }
            out.push(b'e')
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn roundtrip() {
        let mut d = BTreeMap::new();
        d.insert(b"a".to_vec(), Value::Int(42));
        d.insert(b"b".to_vec(), Value::Bytes(b"hello".to_vec()));
        let v = Value::Dict(d);
        let mut e = Vec::new();
        encode(&v, &mut e);
        let (x, n) = decode(&e).unwrap();
        assert_eq!(x, v);
        assert_eq!(n, e.len());
    }
    #[test]
    fn nested() {
        let (v, n) = decode(b"d4:spaml1:a1:bee").unwrap();
        assert_eq!(n, 16);
        assert!(matches!(v, Value::Dict(_)));
    }
}
