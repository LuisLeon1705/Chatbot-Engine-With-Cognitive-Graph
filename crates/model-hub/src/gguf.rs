//! Minimal GGUF header reader: key/value metadata and tensor names, without loading the
//! weights. Arrays (e.g. the vocabulary) are skipped without allocating memory.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, Read, Seek};
use std::path::Path;

const MAGIC: &[u8; 4] = b"GGUF";
/// Sanity limit for strings: a damaged file must not ask for gigabytes of RAM.
const MAX_STR: u64 = 16 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum GgufError {
    #[error("I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("not a GGUF file")]
    BadMagic,
    #[error("damaged GGUF: {0}")]
    Corrupt(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MetaValue {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
    /// Skipped array: only its length.
    Array(u64),
}

#[derive(Debug, Clone, Default)]
pub struct GgufInfo {
    pub version: u32,
    pub tensor_count: u64,
    pub meta: BTreeMap<String, MetaValue>,
    /// Tensor names (only when asked for; they tell diffusion families apart).
    pub tensor_names: Vec<String>,
}

impl GgufInfo {
    pub fn str(&self, key: &str) -> Option<&str> {
        match self.meta.get(key) {
            Some(MetaValue::Str(s)) => Some(s),
            _ => None,
        }
    }

    pub fn int(&self, key: &str) -> Option<i64> {
        match self.meta.get(key) {
            Some(MetaValue::Int(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn architecture(&self) -> Option<&str> {
        self.str("general.architecture")
    }

    /// `<arch>.embedding_length` (the dimension the mmproj's projection must match).
    pub fn embedding_length(&self) -> Option<i64> {
        self.int(&format!("{}.embedding_length", self.architecture()?))
    }

    pub fn context_length(&self) -> Option<i64> {
        self.int(&format!("{}.context_length", self.architecture()?))
    }

    pub fn is_mmproj(&self) -> bool {
        self.architecture() == Some("clip") || matches!(self.str("general.type"), Some("mmproj" | "clip-vision"))
    }

    /// Quantisation name according to `general.file_type` (the `llama_ftype` enum).
    pub fn file_type_name(&self) -> Option<&'static str> {
        Some(match self.int("general.file_type")? {
            0 => "F32",
            1 => "F16",
            2 => "Q4_0",
            3 => "Q4_1",
            7 => "Q8_0",
            8 => "Q5_0",
            9 => "Q5_1",
            10 => "Q2_K",
            11 => "Q3_K_S",
            12 => "Q3_K_M",
            13 => "Q3_K_L",
            14 => "Q4_K_S",
            15 => "Q4_K_M",
            16 => "Q5_K_S",
            17 => "Q5_K_M",
            18 => "Q6_K",
            19 => "IQ2_XXS",
            20 => "IQ2_XS",
            21 => "Q2_K_S",
            22 => "IQ3_XS",
            23 => "IQ3_XXS",
            24 => "IQ1_S",
            25 => "IQ4_NL",
            26 => "IQ3_S",
            27 => "IQ3_M",
            28 => "IQ2_S",
            29 => "IQ2_M",
            30 => "IQ4_XS",
            31 => "IQ1_M",
            32 => "BF16",
            _ => return None,
        })
    }
}

struct R<T: Read + Seek>(T);

impl<T: Read + Seek> R<T> {
    fn bytes<const N: usize>(&mut self) -> Result<[u8; N], GgufError> {
        let mut b = [0u8; N];
        self.0.read_exact(&mut b)?;
        Ok(b)
    }
    fn u32(&mut self) -> Result<u32, GgufError> {
        Ok(u32::from_le_bytes(self.bytes()?))
    }
    fn u64(&mut self) -> Result<u64, GgufError> {
        Ok(u64::from_le_bytes(self.bytes()?))
    }
    fn string(&mut self) -> Result<String, GgufError> {
        let len = self.u64()?;
        if len > MAX_STR {
            return Err(GgufError::Corrupt(format!("string de {len} bytes")));
        }
        let mut buf = vec![0u8; len as usize];
        self.0.read_exact(&mut buf)?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
    fn skip(&mut self, n: u64) -> Result<(), GgufError> {
        self.0.seek_relative(i64::try_from(n).map_err(|_| GgufError::Corrupt("salto enorme".into()))?)?;
        Ok(())
    }

    fn value(&mut self, ty: u32) -> Result<MetaValue, GgufError> {
        Ok(match ty {
            0 => MetaValue::Int(self.bytes::<1>()?[0] as i64),
            1 => MetaValue::Int(self.bytes::<1>()?[0] as i8 as i64),
            2 => MetaValue::Int(u16::from_le_bytes(self.bytes()?) as i64),
            3 => MetaValue::Int(i16::from_le_bytes(self.bytes()?) as i64),
            4 => MetaValue::Int(self.u32()? as i64),
            5 => MetaValue::Int(i32::from_le_bytes(self.bytes()?) as i64),
            6 => MetaValue::Float(f32::from_le_bytes(self.bytes()?) as f64),
            7 => MetaValue::Bool(self.bytes::<1>()?[0] != 0),
            8 => MetaValue::Str(self.string()?),
            9 => {
                let item = self.u32()?;
                let len = self.u64()?;
                self.skip_array(item, len)?;
                MetaValue::Array(len)
            }
            10 => MetaValue::Int(self.u64()? as i64),
            11 => MetaValue::Int(i64::from_le_bytes(self.bytes()?)),
            12 => MetaValue::Float(f64::from_le_bytes(self.bytes()?)),
            t => return Err(GgufError::Corrupt(format!("tipo de valor {t}"))),
        })
    }

    fn skip_array(&mut self, item: u32, len: u64) -> Result<(), GgufError> {
        let fixed = match item {
            0 | 1 | 7 => Some(1),
            2 | 3 => Some(2),
            4..=6 => Some(4),
            10..=12 => Some(8),
            _ => None,
        };
        match (fixed, item) {
            (Some(size), _) => self.skip(len.checked_mul(size).ok_or_else(|| GgufError::Corrupt("array enorme".into()))?),
            (None, 8) => {
                for _ in 0..len {
                    let n = self.u64()?;
                    if n > MAX_STR {
                        return Err(GgufError::Corrupt(format!("string de {n} bytes")));
                    }
                    self.skip(n)?;
                }
                Ok(())
            }
            (None, 9) => {
                for _ in 0..len {
                    let inner = self.u32()?;
                    let n = self.u64()?;
                    self.skip_array(inner, n)?;
                }
                Ok(())
            }
            (None, t) => Err(GgufError::Corrupt(format!("array type {t}"))),
        }
    }
}

/// Reads the header. With `with_tensors` it also reads the tensor names.
pub fn read_gguf(path: &Path, with_tensors: bool) -> Result<GgufInfo, GgufError> {
    read_from(BufReader::with_capacity(1 << 16, File::open(path)?), with_tensors)
}

pub fn read_from<T: Read + Seek>(src: T, with_tensors: bool) -> Result<GgufInfo, GgufError> {
    let mut r = R(src);
    if &r.bytes::<4>()? != MAGIC {
        return Err(GgufError::BadMagic);
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return Err(GgufError::Corrupt(format!("GGUF version {version}")));
    }
    let tensor_count = r.u64()?;
    let kv_count = r.u64()?;
    if kv_count > 1_000_000 || tensor_count > 10_000_000 {
        return Err(GgufError::Corrupt("absurd counts".into()));
    }
    let mut meta = BTreeMap::new();
    for _ in 0..kv_count {
        let key = r.string()?;
        let ty = r.u32()?;
        let v = r.value(ty)?;
        meta.insert(key, v);
    }
    let mut tensor_names = Vec::new();
    if with_tensors {
        tensor_names.reserve(tensor_count as usize);
        for _ in 0..tensor_count {
            let name = r.string()?;
            let dims = r.u32()?;
            if dims > 8 {
                return Err(GgufError::Corrupt(format!("tensor {name} with {dims} dimensions")));
            }
            r.skip(dims as u64 * 8 + 4 + 8)?; // dims u64 + type u32 + offset u64
            tensor_names.push(name);
        }
    }
    Ok(GgufInfo { version, tensor_count, meta, tensor_names })
}

/// Minimal writer, only for tests and tools (header without tensor data).
pub fn write_header(meta: &[(&str, MetaValue)], tensors: &[&str]) -> Vec<u8> {
    let mut out = Vec::new();
    let s = |out: &mut Vec<u8>, v: &str| {
        out.extend_from_slice(&(v.len() as u64).to_le_bytes());
        out.extend_from_slice(v.as_bytes());
    };
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&(tensors.len() as u64).to_le_bytes());
    out.extend_from_slice(&(meta.len() as u64).to_le_bytes());
    for (k, v) in meta {
        s(&mut out, k);
        match v {
            MetaValue::Int(i) => {
                out.extend_from_slice(&11u32.to_le_bytes());
                out.extend_from_slice(&i.to_le_bytes());
            }
            MetaValue::Float(f) => {
                out.extend_from_slice(&12u32.to_le_bytes());
                out.extend_from_slice(&f.to_le_bytes());
            }
            MetaValue::Bool(b) => {
                out.extend_from_slice(&7u32.to_le_bytes());
                out.push(*b as u8);
            }
            MetaValue::Str(v) => {
                out.extend_from_slice(&8u32.to_le_bytes());
                s(&mut out, v);
            }
            MetaValue::Array(n) => {
                // array of "tokN" strings (to test skipping)
                out.extend_from_slice(&9u32.to_le_bytes());
                out.extend_from_slice(&8u32.to_le_bytes());
                out.extend_from_slice(&n.to_le_bytes());
                for i in 0..*n {
                    s(&mut out, &format!("tok{i}"));
                }
            }
        }
    }
    for t in tensors {
        s(&mut out, t);
        out.extend_from_slice(&1u32.to_le_bytes());
        out.extend_from_slice(&1u64.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&0u64.to_le_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_metadata_skipping_arrays_and_tensors() {
        let bytes = write_header(
            &[
                ("general.architecture", MetaValue::Str("gemma4".into())),
                ("tokenizer.ggml.tokens", MetaValue::Array(1000)),
                ("gemma4.embedding_length", MetaValue::Int(3840)),
                ("general.file_type", MetaValue::Int(15)),
            ],
            &["token_embd.weight", "blk.0.attn_q.weight"],
        );
        let info = read_from(std::io::Cursor::new(&bytes), true).unwrap();
        assert_eq!(info.architecture(), Some("gemma4"));
        assert_eq!(info.embedding_length(), Some(3840));
        assert_eq!(info.meta["tokenizer.ggml.tokens"], MetaValue::Array(1000));
        assert_eq!(info.file_type_name(), Some("Q4_K_M"));
        assert_eq!(info.tensor_names, ["token_embd.weight", "blk.0.attn_q.weight"]);
        assert!(read_from(std::io::Cursor::new(&bytes[..bytes.len() / 2]), true).is_err());
        assert!(matches!(read_from(std::io::Cursor::new(b"JSON...."), false), Err(GgufError::BadMagic)));
    }
}
