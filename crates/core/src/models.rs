use anyhow::{bail, Context, Result};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const DEFAULT_BASE_URL: &str = "https://huggingface.co";

#[derive(Debug, Clone, Deserialize)]
pub struct ModelFile { pub name: String, pub path: String, pub sha256: String }

#[derive(Debug, Clone, Deserialize)]
pub struct ModelSpec { pub files: Vec<ModelFile> }

pub fn manifest() -> BTreeMap<String, ModelSpec> {
    serde_json::from_str(include_str!("../models.json")).expect("models.json válido")
}

pub trait Fetch {
    fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)>;
}

pub struct HttpFetch;

impl Fetch for HttpFetch {
    fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)> {
        let resp = reqwest::blocking::Client::builder()
            .timeout(None)
            .build()?
            .get(url)
            .send()?
            .error_for_status()
            .with_context(|| format!("baixar {url}"))?;
        let len = resp.content_length();
        Ok((Box::new(resp), len))
    }
}

fn file_sha(path: &Path) -> Result<String> {
    let mut h = Sha256::new();
    std::io::copy(&mut File::open(path)?, &mut h)?;
    Ok(hex::encode(h.finalize()))
}

pub fn ensure(
    spec: &ModelSpec, base_url: &str, dir: &Path, fetch: &dyn Fetch,
    progress: &mut dyn FnMut(&str, u64, Option<u64>),
) -> Result<PathBuf> {
    fs::create_dir_all(dir)?;
    for f in &spec.files {
        let dest = dir.join(&f.name);
        if dest.exists() && file_sha(&dest)? == f.sha256 {
            // Arquivo já válido: um evento final para a interface mostrá-lo como completo.
            let len = fs::metadata(&dest)?.len();
            progress(&f.name, len, Some(len));
            continue;
        }
        let part = dir.join(format!("{}.part", f.name));
        let url = format!("{}/{}", base_url.trim_end_matches('/'), f.path);
        let (mut body, total) = fetch.open(&url)?;
        let mut out = File::create(&part)?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 1 << 16];
        let mut done = 0u64;
        loop {
            let n = body.read(&mut buf)?;
            if n == 0 { break; }
            out.write_all(&buf[..n])?;
            hasher.update(&buf[..n]);
            done += n as u64;
            progress(&f.name, done, total);
        }
        out.sync_all()?;
        drop(out);
        let got = hex::encode(hasher.finalize());
        if got != f.sha256 {
            let _ = fs::remove_file(&part);
            bail!("hash inválido para {}: esperado {}, obtido {}", f.name, f.sha256, got);
        }
        fs::rename(&part, &dest)?;
    }
    Ok(dir.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::collections::HashMap;

    struct FakeFetch { files: HashMap<String, Vec<u8>>, calls: Cell<usize> }
    impl Fetch for FakeFetch {
        fn open(&self, url: &str) -> Result<(Box<dyn Read>, Option<u64>)> {
            self.calls.set(self.calls.get() + 1);
            let data = self.files.get(url).cloned().ok_or_else(|| anyhow::anyhow!("404 {url}"))?;
            let len = data.len() as u64;
            Ok((Box::new(std::io::Cursor::new(data)), Some(len)))
        }
    }

    fn sha(b: &[u8]) -> String { hex::encode(Sha256::digest(b)) }

    fn spec(sha256: String) -> ModelSpec {
        ModelSpec { files: vec![ModelFile { name: "m.bin".into(), path: "org/m.bin".into(), sha256 }] }
    }

    fn fetch() -> FakeFetch {
        FakeFetch { files: HashMap::from([("https://hf.test/org/m.bin".to_string(), b"modelo".to_vec())]),
                    calls: Cell::new(0) }
    }

    #[test]
    fn downloads_and_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let f = fetch();
        let mut seen = 0;
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &f, &mut |name, n, _| { assert_eq!(name, "m.bin"); seen = n }).unwrap();
        assert_eq!(std::fs::read(dir.path().join("m.bin")).unwrap(), b"modelo");
        assert_eq!(seen, 6);
    }

    #[test]
    fn skips_valid_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin"), b"modelo").unwrap();
        let f = fetch();
        let mut ev = vec![];
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &f,
               &mut |name, d, t| ev.push((name.to_string(), d, t))).unwrap();
        assert_eq!(f.calls.get(), 0);
        assert_eq!(ev, vec![("m.bin".to_string(), 6, Some(6))]);
    }

    #[test]
    fn wrong_hash_fails_and_leaves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let err = ensure(&spec(sha(b"outro")), "https://hf.test", dir.path(), &fetch(), &mut |_, _, _| {});
        assert!(err.is_err());
        assert!(std::fs::read_dir(dir.path()).unwrap().next().is_none());
    }

    #[test]
    fn corrupt_existing_file_is_replaced() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("m.bin"), b"corrompido").unwrap();
        ensure(&spec(sha(b"modelo")), "https://hf.test", dir.path(), &fetch(), &mut |_, _, _| {}).unwrap();
        assert_eq!(std::fs::read(dir.path().join("m.bin")).unwrap(), b"modelo");
    }

    #[test]
    fn manifest_has_expected_ids_and_real_hashes() {
        let m = manifest();
        for id in ["whisper-small-en", "whisper-turbo", "opus-mt-en-pt"] {
            let spec = m.get(id).unwrap_or_else(|| panic!("falta {id}"));
            assert!(spec.files.iter().all(|f| f.sha256.len() == 64), "hash ausente em {id}");
        }
    }
}
