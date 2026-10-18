//! `.safetensors` header: a u64 LE length + JSON with the tensors.
//! Only the names are read (to detect the family of a diffusion model).

use std::fs::File;
use std::io::Read;
use std::path::Path;

const MAX_HEADER: u64 = 100 * 1024 * 1024;

pub fn tensor_names(path: &Path) -> std::io::Result<Vec<String>> {
    let mut f = File::open(path)?;
    let mut len = [0u8; 8];
    f.read_exact(&mut len)?;
    let len = u64::from_le_bytes(len);
    if len == 0 || len > MAX_HEADER {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, format!("safetensors header of {len} bytes")));
    }
    let mut buf = vec![0u8; len as usize];
    f.read_exact(&mut buf)?;
    let header: serde_json::Map<String, serde_json::Value> =
        serde_json::from_slice(&buf).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    Ok(header.into_iter().map(|(k, _)| k).filter(|k| k != "__metadata__").collect())
}

/// Header writer for tests.
pub fn write_header(names: &[&str]) -> Vec<u8> {
    let map: serde_json::Map<String, serde_json::Value> = names
        .iter()
        .map(|n| (n.to_string(), serde_json::json!({ "dtype": "F16", "shape": [1], "data_offsets": [0, 2] })))
        .collect();
    let json = serde_json::to_vec(&map).unwrap();
    let mut out = (json.len() as u64).to_le_bytes().to_vec();
    out.extend_from_slice(&json);
    out.extend_from_slice(&[0, 0]);
    out
}
