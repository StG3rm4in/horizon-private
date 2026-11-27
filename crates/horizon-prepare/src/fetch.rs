//! Fetches the pinned Mojang files the player's own copy may lack: the version JSON, the client
//! and server jars, the libraries and single asset objects. Every file is checked against the
//! SHA-1 research-lock.json or the version JSON pins and published through `files::Import`, so a
//! download is indistinguishable from an import once it lands. Off with `--no-download`, or
//! without the `download` feature.
//!
//! An interrupted transfer keeps its bytes under `.local/downloads/<algorithm>-<checksum>.part`
//! and the next call, in this run or a later one, asks the server for the rest with `Range`. The
//! name says which file the bytes are *meant* for, not that they are right: the hash decides, and
//! a mismatch drops the partial and starts over.
//!
//! The cache is shared by every entry point of a root, so each transfer locks it itself, whatever
//! lock its caller holds. `HORIZON_MOJANG_META` and `HORIZON_MOJANG_OBJECTS` point the bases at a
//! mirror (the tests use a local one).
//!
//! These are the only Mojang hosts named outside the launcher's mojang.rs.

use crate::files::Import;
use crate::Log;
use sha1::Digest;
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Tries per file before giving up; the partial file stays for the next call.
const ATTEMPTS: u32 = 3;
/// How much is written between two progress lines.
const PROGRESS_STEP: u64 = 8 << 20;
/// A version JSON's ceiling: the documents are tens of kilobytes.
const MAX_DOCUMENT: u64 = 8 << 20;
const META: &str = "https://piston-meta.mojang.com/v1/packages";
const OBJECTS: &str = "https://resources.download.minecraft.net";

/// A running checksum of the bytes written so far.
enum Running {
    Sha1(sha1::Sha1),
    Sha256(sha2::Sha256),
}

impl Running {
    fn new(algorithm: &str) -> Self {
        if algorithm == "sha1" {
            Self::Sha1(sha1::Sha1::new())
        } else {
            Self::Sha256(sha2::Sha256::new())
        }
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(hash) => hash.update(data),
            Self::Sha256(hash) => hash.update(data),
        }
    }

    fn finish(self) -> String {
        match self {
            Self::Sha1(hash) => crate::hex(&hash.finalize()),
            Self::Sha256(hash) => crate::hex(&hash.finalize()),
        }
    }
}

fn hex_digest(data: &[u8], algorithm: &str) -> String {
    if algorithm == "sha1" {
        crate::hex(&sha1::Sha1::digest(data))
    } else {
        crate::hex(&sha2::Sha256::digest(data))
    }
}

fn agent() -> ureq::Agent {
    let mut config = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(600)))
        .user_agent(concat!("horizon-prepare/", env!("CARGO_PKG_VERSION")));
    if let Some(proxy) = proxy_from_environment() {
        config = config.proxy(Some(proxy));
    }
    config.build().new_agent()
}

fn proxy_from_environment() -> Option<ureq::Proxy> {
    ["HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy"]
        .iter()
        .filter_map(|name| std::env::var(name).ok())
        .map(|value| value.trim().to_string())
        .find(|value| !value.is_empty())
        .and_then(|value| ureq::Proxy::new(&value).ok())
}

/// The HTTPS side: one agent for every file a preparation needs.
pub(crate) struct Fetch {
    cache: PathBuf,
    meta: String,
    objects: String,
    agent: ureq::Agent,
}

impl Fetch {
    pub(crate) fn with_bases(root: &Path, meta: &str, objects: &str) -> Self {
        Self {
            cache: root.join(".local/downloads"),
            meta: meta.trim_end_matches('/').to_string(),
            objects: objects.trim_end_matches('/').to_string(),
            agent: agent(),
        }
    }

    pub(crate) fn new(root: &Path) -> Self {
        let base = |name: &str, default: &str| {
            std::env::var(name)
                .ok()
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| default.to_string())
        };
        Self::with_bases(
            root,
            &base("HORIZON_MOJANG_META", META),
            &base("HORIZON_MOJANG_OBJECTS", OBJECTS),
        )
    }

    /// The pinned version JSON's URL: Mojang serves it under the SHA-1 that pins it.
    pub(crate) fn manifest_url(&self, checksum: &str, version: &str) -> String {
        format!("{}/{checksum}/{version}.json", self.meta)
    }

    /// An asset object's URL, laid out by its SHA-1.
    pub(crate) fn object_url(&self, checksum: &str) -> String {
        format!(
            "{}/{}/{}",
            self.objects,
            checksum.get(..2).unwrap_or(checksum),
            checksum
        )
    }

    /// The asset object `checksum`.
    pub(crate) fn object(
        &self,
        checksum: &str,
        size: u64,
        destination: &Path,
        log: Log,
    ) -> Result<(), String> {
        self.file(
            &self.object_url(checksum),
            checksum,
            "sha1",
            size,
            destination,
            log,
        )
    }

    /// A small document in memory (the version JSON): the pinned SHA-1 is its only check.
    pub(crate) fn bytes(
        &self,
        url: &str,
        checksum: &str,
        algorithm: &str,
    ) -> Result<Vec<u8>, String> {
        let mut failure = String::new();
        for attempt in 1..=ATTEMPTS {
            if attempt > 1 {
                std::thread::sleep(Duration::from_secs(1 << (attempt - 2)));
            }
            let fetched = (|| {
                let mut response = self.agent.get(url).call().map_err(|e| e.to_string())?;
                let mut data = Vec::new();
                response
                    .body_mut()
                    .as_reader()
                    .take(MAX_DOCUMENT)
                    .read_to_end(&mut data)
                    .map_err(|e| e.to_string())?;
                let actual = hex_digest(&data, algorithm);
                if actual != checksum {
                    return Err(format!("expected {algorithm} {checksum}, got {actual}"));
                }
                Ok(data)
            })();
            match fetched {
                Ok(data) => return Ok(data),
                Err(error) => failure = error,
            }
        }
        Err(format!("{url}: {failure}"))
    }

    /// Fetches `url` into `destination`, published only once its bytes match `checksum`
    /// (`sha1` or `sha256`) and there are `size` of them.
    pub(crate) fn file(
        &self,
        url: &str,
        checksum: &str,
        algorithm: &str,
        size: u64,
        destination: &Path,
        log: Log,
    ) -> Result<(), String> {
        if destination.is_file() && crate::digest(destination, algorithm)? == checksum {
            return Ok(());
        }
        // The cache is shared by every entry point of this root, whichever folder they lock:
        // serialize on the cache itself rather than on a caller's discipline.
        let _lock = crate::hold
