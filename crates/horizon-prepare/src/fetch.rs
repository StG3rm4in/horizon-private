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
#[derive(Clone)]
pub struct Fetch {
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

    pub fn new(root: &Path) -> Self {
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
        let _lock = crate::hold_lock(&self.cache, log)?;
        let partial = self.cache.join(format!("{algorithm}-{checksum}.part"));
        log(&format!("Fetching {url} ({:.1} MB)", size as f64 / 1e6));
        let mut failure = String::new();
        for attempt in 1..=ATTEMPTS {
            if attempt > 1 {
                log(&format!("Retrying {url} after {failure}"));
                std::thread::sleep(Duration::from_secs(1 << (attempt - 2)));
            }
            match self.attempt(url, checksum, algorithm, size, &partial, log) {
                Ok(()) => {
                    fs::create_dir_all(destination.parent().unwrap())
                        .map_err(|e| format!("{}: {e}", destination.display()))?;
                    // `Import` re-reads and checks the file it publishes; the partial stays
                    // behind until it does, so a failure here is still resumable.
                    Import::stage(&partial, destination, checksum, algorithm, false)?.commit()?;
                    let _ = fs::remove_file(&partial);
                    log(&format!("Fetched {url}"));
                    return Ok(());
                }
                Err(error) => failure = error,
            }
        }
        Err(format!("{url}: {failure}"))
    }

    /// One transfer, resuming from the partial file's bytes.
    fn attempt(
        &self,
        url: &str,
        checksum: &str,
        algorithm: &str,
        size: u64,
        partial: &Path,
        log: Log,
    ) -> Result<(), String> {
        let error = |e: std::io::Error| format!("{}: {e}", partial.display());
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(partial)
            .map_err(error)?;
        let mut offset = file.metadata().map_err(error)?.len();
        // More bytes than the file has can only be a stale partial: start over.
        if offset > size {
            file.set_len(0).map_err(error)?;
            offset = 0;
        }
        let mut hash = Running::new(algorithm);
        if offset > 0 {
            let mut prefix = fs::File::open(partial).map_err(error)?;
            let mut buffer = vec![0; 64 * 1024];
            let mut left = offset;
            while left > 0 {
                let want = buffer.len().min(left as usize);
                let n = prefix.read(&mut buffer[..want]).map_err(error)?;
                if n == 0 {
                    break;
                }
                hash.update(&buffer[..n]);
                left -= n as u64;
            }
        }
        let mut response = loop {
            let mut request = self.agent.get(url);
            if offset > 0 {
                let range = format!("bytes={offset}-");
                request = request.header("Range", &range);
            }
            match request.call() {
                Ok(response) => break response,
                // The partial is no prefix of this file any more.
                Err(ureq::Error::StatusCode(416)) if offset > 0 => {
                    file.set_len(0).map_err(error)?;
                    offset = 0;
                    hash = Running::new(algorithm);
                }
                Err(failure) => return Err(failure.to_string()),
            }
        };
        // A server that ignores the range answers 200 with the whole file.
        if response.status().as_u16() == 200 && offset > 0 {
            file.set_len(0).map_err(error)?;
            offset = 0;
            hash = Running::new(algorithm);
        }
        let mut written = offset;
        let mut reported = written;
        let mut reader = response.body_mut().as_reader();
        let mut buffer = vec![0; 64 * 1024];
        loop {
            let n = reader.read(&mut buffer).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            written += n as u64;
            if written > size {
                return Err(format!("more than the {size} pinned bytes"));
            }
            file.write_all(&buffer[..n]).map_err(error)?;
            hash.update(&buffer[..n]);
            if written - reported >= PROGRESS_STEP {
                log(&format!("{url}: {written} of {size} bytes"));
                reported = written;
            }
        }
        file.flush().map_err(error)?;
        // Closed before the removal below: our own handle shares delete, but a handle held by
        // something else need not, and the sequence stays linear.
        drop(file);
        if written < size {
            return Err(format!("stopped at {written} of {size} bytes"));
        }
        let actual = hash.finish();
        if actual != checksum {
            // Wrong bytes cannot be resumed: the next attempt starts clean.
            let _ = fs::remove_file(partial);
            return Err(format!("expected {algorithm} {checksum}, got {actual}"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Copy, PartialEq)]
    enum Mode {
        Plain,
        /// Each response stops after an eighth of what is left, as a dropped connection does.
        Cut,
        /// A ranged request is answered with 416, as a partial that is no prefix is.
        RejectRange,
    }

    struct Fixture {
        dir: PathBuf,
        root: PathBuf,
        url: String,
        data: Vec<u8>,
        mode: Arc<AtomicUsize>,
        requests: Arc<AtomicUsize>,
        ranges: Arc<Mutex<Vec<String>>>,
        server: Arc<tiny_http::Server>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Fixture {
        fn new(name: &str, mode: Mode) -> Self {
            Self::sized(name, mode, 400_000)
        }

        fn sized(name: &str, mode: Mode, len: u32) -> Self {
            let dir =
                std::env::temp_dir().join(format!("horizon-fetch-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            let root = dir.join("root");
            fs::create_dir_all(&root).unwrap();
            let data: Vec<u8> = (0..len).map(|n| (n % 251) as u8).collect();
            let server = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let address = server.server_addr().to_ip().unwrap();
            let url = format!("http://{address}/file");
            let mode = Arc::new(AtomicUsize::new(mode as usize));
            let requests = Arc::new(AtomicUsize::new(0));
            let ranges = Arc::new(Mutex::new(Vec::new()));
            let (served, shared, seen_mode, seen, seen_ranges) = (
                server.clone(),
                Arc::new(data.clone()),
                mode.clone(),
                requests.clone(),
                ranges.clone(),
            );
            let thread = std::thread::spawn(move || {
                while let Ok(request) = served.recv() {
                    seen.fetch_add(1, Ordering::Relaxed);
                    let range = request
                        .headers()
                        .iter()
                        .find(|h| h.field.equiv("Range"))
                        .map(|h| h.value.as_str().to_string());
                    if let Some(range) = &range {
                        seen_ranges.lock().unwrap().push(range.clone());
                    }
                    let mode = match seen_mode.load(Ordering::Relaxed) {
                        1 => Mode::Cut,
                        2 => Mode::RejectRange,
                        _ => Mode::Plain,
                    };
                    respond(request, &shared, mode, range.as_deref());
                }
            });
            Fixture {
                dir,
                root,
                url,
                data,
                mode,
                requests,
                ranges,
                server,
                thread: Some(thread),
            }
        }

        fn sha1(&self) -> String {
            crate::hex(&sha1::Sha1::digest(&self.data))
        }

        fn destination(&self) -> PathBuf {
            self.dir.join("out/client.jar")
        }

        fn partial_for(&self, checksum: &str) -> PathBuf {
            self.root
                .join(".local/downloads")
                .join(format!("sha1-{checksum}.part"))
        }

        fn partial(&self) -> PathBuf {
            self.partial_for(&self.sha1())
        }

        fn fetch(&self) -> Fetch {
            Fetch::with_bases(&self.root, "http://127.0.0.1:1", "http://127.0.0.1:1")
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            self.server.unblock();
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
            let _ = fs::remove_dir_all(&self.dir);
        }
    }

    fn respond(request: tiny_http::Request, data: &[u8], mode: Mode, range: Option<&str>) {
        let total = data.len();
        let start = range
            .and_then(|r| r.strip_prefix("bytes="))
            .and_then(|r| r.split('-').next())
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        if mode == Mode::RejectRange && range.is_some() {
            let _ = request.respond(tiny_http::Response::empty(416));
            return;
        }
        let ranged = start > 0 && start < total;
        let start = if ranged { start } else { 0 };
        let rest = &data[start..];
        let mut headers =
            vec![tiny_http::Header::from_bytes(&b"Connection"[..], &b"close"[..]).unwrap()];
        if ranged {
            let value = format!("bytes {start}-{}/{total}", total - 1);
            headers
                .push(tiny_http::Header::from_bytes(&b"Content-Range"[..], value.as_bytes()).unwrap());
        }
        // The advertised length stays the real one: a short body is what a cut connection is.
        let limit = match mode {
            Mode::Cut => (rest.len() / 8).max(1),
            _ => rest.len(),
        };
        let status = if ranged { 206 } else { 200 };
        let response = tiny_http::Response::new(
            tiny_http::StatusCode(status),
            headers,
            &rest[..limit.min(rest.len())],
            Some(rest.len()),
            None,
        );
        let _ = request.respond(response);
    }

    #[test]
    fn a_pinned_file_is_fetched_checked_and_published() {
        let f = Fixture::new("whole", Mode::Plain);
        let lines = RefCell::new(Vec::new());
        f.fetch()
            .file(
                &f.url,
                &f.sha1(),
                "sha1",
                f.data.len() as u64,
                &f.destination(),
                &|line| lines.borrow_mut().push(line.to_string()),
            )
            .unwrap();
        assert_eq!(fs::read(f.destination()).unwrap(), f.data);
        assert!(!f.partial().exists());
        assert_eq!(f.requests.load(Ordering::Relaxed), 1);
        let lines = lines.into_inner();
        assert_eq!(lines.len(), 2, "{lines:?}");
        assert!(lines[0].starts_with("Fetching "), "{lines:?}");
        assert!(lines[1].starts_with("Fetched "), "{lines:?}");
        // Already published: no second request.
        f.fetch()
            .file(
                &f.url,
                &f.sha1(),
                "sha1",
                f.data.len() as u64,
                &f.destination(),
                &|_| {},
            )
            .unwrap();
        assert_eq!(f.requests.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn progress_is_reported_for_a_file_bigger_than_a_step() {
        let f = Fixture::sized("progress", Mode::Plain, PROGRESS_STEP as u32 + 100_000);
        let lines = RefCell::new(Vec::new());
        f.fetch()
            .file(
                &f.url,
                &f.sha1(),
                "sha1",
                f.data.len() as u64,
                &f.destination(),
                &|line| lines.borrow_mut().push(line.to_string()),
            )
            .unwrap();
        let lines = lines.into_inner();
        assert!(lines.iter().any(|line| line.contains(" of ")), "{lines:?}");
        assert!(lines.last().unwrap().starts_with("Fetched "), "{lines:?}");
    }

    #[test]
    fn an_interrupted_download_resumes_in_the_next_call() {
        let f = Fixture::new("resume", Mode::Cut);
        let (checksum, size) = (f.sha1(), f.data.len() as u64);
        assert!(f
            .fetch()
            .file(&f.url, &checksum, "sha1", size, &f.destination(), &|_| {})
            .is_err());
        // Nothing is published and the received bytes are kept.
        assert!(!f.destination().exists());
        let kept = fs::metadata(f.partial()).unwrap().len();
        assert!(kept > 0 && kept < size, "{kept}");
        f.mode.store(Mode::Plain as usize, Ordering::Relaxed);
        f.fetch()
            .file(&f.url, &checksum, "sha1", size, &f.destination(), &|_| {})
            .unwrap();
        assert_eq!(fs::read(f.destination()).unwrap(), f.data);
        assert!(!f.partial().exists());
        let ranges = f.ranges.lock().unwrap().clone();
        assert!(ranges.contains(&format!("bytes={kept}-")), "{ranges:?}");
    }

    #[test]
    fn bytes_that_do_not_match_are_not_published() {
        let f = Fixture::new("wrong", Mode::Plain);
        let wrong = "0".repeat(40);
        let size = f.data.len() as u64;
        let error = f
            .fetch()
            .file(&f.url, &wrong, "sha1", size, &f.destination(), &|_| {})
            .unwrap_err();
        assert!(error.contains("expected sha1"), "{error}");
        assert!(!f.destination().exists());
        assert!(!f.partial_for(&wrong).exists(), "the rejected bytes are dropped");
        assert!(!f.partial().exists(), "no partial under the pinned name");
    }

    #[test]
    fn a_partial_that_is_no_prefix_starts_over() {
        let f = Fixture::new("stale", Mode::RejectRange);
        fs::create_dir_all(f.partial().parent().unwrap()).unwrap();
        fs::write(f.partial(), b"a previous revision").unwrap();
        f.fetch()
            .file(
                &f.url,
                &f.sha1(),
                "sha1",
                f.data.len() as u64,
                &f.destination(),
                &|_| {},
            )
            .unwrap();
        assert_eq!(fs::read(f.destination()).unwrap(), f.data);
        let ranges = f.ranges.lock().unwrap().clone();
        assert_eq!(ranges, ["bytes=18-"], "{ranges:?}");
    }

    #[test]
    fn a_document_is_read_in_memory_and_checked() {
        let f = Fixture::new("document", Mode::Plain);
        let checksum = f.sha1();
        let data = f
            .fetch()
            .bytes(&f.url, &checksum, "sha1")
            .unwrap();
        assert_eq!(data, f.data);
        assert!(f
            .fetch()
            .bytes(&f.url, &"0".repeat(40), "sha1")
            .is_err());
        assert_eq!(f.requests.load(Ordering::Relaxed), 4, "three tries for the wrong hash");
    }
}
