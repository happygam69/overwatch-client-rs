//! Opt-in CDN access: fetch content the local install does not carry.
//!
//! Everything here is **off unless `OW_CDN` is set** to `1`, `on` or `true`.
//! With it set, the store may fetch a file it could not resolve locally from
//! Blizzard's CDN — read-only, never touching the installation:
//!
//! * `OW_CDN=1` enables fetching.
//! * `OW_CDN_CACHE=<dir>` selects where fetched bytes are cached (default
//!   `target/cdn-cache`, created on demand).
//! * `OW_CDN_TIMEOUT=<seconds>` sets the connect and read timeout (default 60).
//!
//! Routes, per TACTLib's `HttpCDNClient.Fetch` and `ICDNClient`:
//!
//! ```text
//! data   {base}/{cdn_path}/data/{k0k1}/{k2k3}/{key}{suffix}   (whole or Range GET)
//! config {base}/{cdn_path}/config/{k0k1}/{k2k3}/{key}         (whole GET)
//! ```
//!
//! `{base}` is a CDN host from `.build.info` (`CDN Hosts`), bare or with a
//! scheme; a bare host gets `http://` prepended, exactly as the reference does.
//! The key is lowercase hex and the suffix is usually empty or `.index`.
//! Archive slices are Range GETs on the **archive's own hash**, not on the
//! key being fetched: TACTLib's `FetchIndexEntry` turns an index entry into
//! `Range: bytes=offset-(offset+size-1)` on `data/…/<archive hash>`.
//!
//! There is **no TLS** (no dependency may be added for it): an `https://` host,
//! or a redirect to one, is refused with [`Error::TlsRequired`] rather than
//! attempting something the OS might silently downgrade.

use std::fmt;
use std::fs;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::container::IndexFooter;

/// Response bodies larger than this are refused: a data file that big is a bug
/// or a hostile answer, not content.
const MAX_BODY: u64 = 1 << 30;
const MAX_REDIRECTS: usize = 3;

#[derive(Debug)]
pub enum Error {
    /// The key is not well-formed (hex, 16 bytes).
    BadKey(String),
    /// No CDN host is configured.
    NoHosts,
    /// A URL or redirect target this client will not use.
    TlsRequired(String),
    BadUrl(String),
    /// The server answered with a code other than 200/206/3xx.
    Status(u16, String),
    /// The server answered 404 after every host was tried.
    NotFound(String),
    /// A response could not be parsed (headers, chunking, framing).
    BadResponse(String),
    /// Redirects exceeded [`MAX_REDIRECTS`].
    TooManyRedirects(String),
    Io(std::io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::BadKey(k) => write!(f, "{k:?} is not a 16-byte hex key"),
            Error::NoHosts => write!(f, "no CDN host is configured"),
            Error::TlsRequired(u) => {
                write!(
                    f,
                    "{u} needs TLS, which this build has no dependency for; refusing"
                )
            }
            Error::BadUrl(u) => write!(f, "unusable CDN URL: {u}"),
            Error::Status(c, u) => write!(f, "CDN answered {c} for {u}"),
            Error::NotFound(u) => write!(f, "CDN has no file at {u}"),
            Error::BadResponse(m) => write!(f, "bad CDN response: {m}"),
            Error::TooManyRedirects(u) => write!(f, "more than {MAX_REDIRECTS} redirects for {u}"),
            Error::Io(e) => write!(f, "io error: {e}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// Is fetching enabled by the environment?
pub fn enabled() -> bool {
    matches!(
        std::env::var("OW_CDN")
            .ok()
            .map(|v| v.trim().to_ascii_lowercase())
            .as_deref(),
        Some("1") | Some("on") | Some("true")
    )
}

/// A read-only client for one build's CDN space.
#[derive(Debug, Clone)]
pub struct CdnClient {
    /// Normalized bases: `http://host[:port]`, no trailing slash.
    hosts: Vec<String>,
    /// URL path segment: the build's `CDN Path` (`tpr/ovw`), no slashes.
    cdn_path: String,
    /// Where fetched bytes are cached; `None` disables the cache.
    cache_dir: Option<PathBuf>,
    timeout: Duration,
}

impl CdnClient {
    /// Build a client from parsed `.build.info` columns, reading the
    /// `OW_CDN_*` environment for cache directory and timeout.
    ///
    /// Only called when [`enabled`] is true; not being able to create the
    /// cache directory is an error, because silently fetching without a cache
    /// would make every run re-download the same bytes.
    pub fn from_env(hosts: &[String], cdn_path: &str) -> Result<CdnClient> {
        let cache_dir = std::env::var("OW_CDN_CACHE")
            .ok()
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target").join("cdn-cache"));
        let timeout_secs = std::env::var("OW_CDN_TIMEOUT")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .filter(|s| *s > 0)
            .unwrap_or(60);
        CdnClient::new(
            hosts,
            cdn_path,
            Some(cache_dir),
            Duration::from_secs(timeout_secs),
        )
    }

    /// Build a client directly (tests use this to point at a local mock).
    pub fn new(
        hosts: &[String],
        cdn_path: &str,
        cache_dir: Option<PathBuf>,
        timeout: Duration,
    ) -> Result<CdnClient> {
        let hosts: Vec<String> = hosts
            .iter()
            .map(|h| normalize_host(h))
            .collect::<Result<_>>()?;
        if hosts.is_empty() {
            return Err(Error::NoHosts);
        }
        let cdn_path = cdn_path.trim().trim_matches('/').to_string();
        if let Some(dir) = &cache_dir {
            fs::create_dir_all(dir)?;
        }
        Ok(CdnClient {
            hosts,
            cdn_path,
            cache_dir,
            timeout,
        })
    }

    /// The hosts this client will try, in order.
    pub fn hosts(&self) -> &[String] {
        &self.hosts
    }

    /// The cache directory, if any.
    pub fn cache_dir(&self) -> Option<&Path> {
        self.cache_dir.as_deref()
    }

    /// One-time notice that remote fetching is live, so a fetch is never
    /// silent — the operator asked for it via `OW_CDN`.
    pub fn announce(&self) {
        static ANNOUNCED: AtomicBool = AtomicBool::new(false);
        if ANNOUNCED.swap(true, Ordering::Relaxed) {
            return;
        }
        eprintln!(
            "ow-casc: CDN enabled (OW_CDN); hosts {}; cache {}; \
             fetching missing content from Blizzard's CDN, read-only",
            self.hosts.join(", "),
            self.cache_dir
                .as_ref()
                .map(|d| d.display().to_string())
                .unwrap_or_else(|| "(none)".to_string()),
        );
    }

    /// `data` route: fetch a whole file by key, cache first.
    ///
    /// Used for loose files — the ones the CDN's `file-index` lists — and for
    /// the `.index` suffix variant of index files.
    pub fn fetch_data(&self, key: &str, suffix: Option<&str>) -> Result<Vec<u8>> {
        let key = key.to_ascii_lowercase();
        validate_key(&key)?;
        let cache = self.cache_dir.as_ref().map(|d| {
            d.join("data")
                .join(&key[0..2])
                .join(&key[2..4])
                .join(format!("{key}{}", suffix.unwrap_or("")))
        });
        if let Some(path) = &cache {
            if let Ok(bytes) = fs::read(path) {
                return Ok(bytes);
            }
        }
        let bytes = self.fetch_over_hosts("data", &key, suffix, None)?;
        if let Some(path) = &cache {
            store_cache(path, &bytes);
        }
        Ok(bytes)
    }

    /// `config` route: fetch a config file by key, cache first.
    ///
    /// Config files are served as plain text; the reference's `FetchConfig`
    /// does not decode anything here, so neither do we.
    pub fn fetch_config(&self, key: &str) -> Result<Vec<u8>> {
        let key = key.to_ascii_lowercase();
        validate_key(&key)?;
        let cache = self.cache_dir.as_ref().map(|d| {
            d.join("config")
                .join(&key[0..2])
                .join(&key[2..4])
                .join(&key)
        });
        if let Some(path) = &cache {
            if let Ok(bytes) = fs::read(path) {
                return Ok(bytes);
            }
        }
        let bytes = self.fetch_over_hosts("config", &key, None, None)?;
        if let Some(path) = &cache {
            store_cache(path, &bytes);
        }
        Ok(bytes)
    }

    /// `data` route with a Range header: fetch `len` bytes at `offset` of the
    /// file named by `key` (an archive hash, for slices).
    ///
    /// Not cached: archive slices are read once and are a fraction of the
    /// archive, and an obfuscated on-disk cache layout for them is complexity
    /// with nothing to show for it.
    pub fn fetch_range(&self, key: &str, offset: u64, len: u64) -> Result<Vec<u8>> {
        self.fetch_data_range(key, None, offset, len)
    }

    /// `data` route Range with the `.index` suffix, the client's
    /// `CdnPathFormat` mode 1. Used for the footer, tail and page spans of an
    /// index file.
    fn fetch_data_range(
        &self,
        key: &str,
        suffix: Option<&str>,
        offset: u64,
        len: u64,
    ) -> Result<Vec<u8>> {
        let key = key.to_ascii_lowercase();
        validate_key(&key)?;
        if len == 0 {
            return Ok(Vec::new());
        }
        self.fetch_over_hosts(
            "data",
            &key,
            suffix,
            Some((offset, offset + len.saturating_sub(1))),
        )
    }

    /// The 20-byte footer at the end of `indices/<key>.index`.
    ///
    /// The client's handler knows the file length from the CDN config's
    /// `*-index-size` field and reads the last 20 bytes first; it does not ask
    /// the server for the length. This is `CdnPathFormat` mode 1 followed by
    /// the Range GET the downloader builds from `offset`/`length`
    /// (`0x1424c1030`'s `Range: bytes=<start>-<end>`).
    pub fn fetch_index_footer(&self, key: &str, file_len: u64) -> Result<[u8; 20]> {
        if file_len < 20 {
            return Err(Error::BadResponse(format!(
                "index {key} is {file_len} B, shorter than its 20-byte footer"
            )));
        }
        let bytes = self.fetch_data_range(key, Some(".index"), file_len - 20, 20)?;
        bytes
            .as_slice()
            .try_into()
            .map_err(|_| Error::BadResponse(format!("index footer is {} B", bytes.len())))
    }

    /// The bytes after the page bodies: the last-key array, the per-page
    /// checksums and the terminal checksum.
    pub fn fetch_index_tail(&self, key: &str, footer: &IndexFooter) -> Result<Vec<u8>> {
        let offset = footer.page_offset(footer.page_count);
        let len = u64::from(footer.tail_len());
        self.fetch_data_range(key, Some(".index"), offset, len)
    }

    /// Page `page` of an index: `pageSize` bytes at `page * pageSize`.
    ///
    /// The client's page layout puts page `p` at `p * pageSize`; the page body
    /// is always the full `pageSize`, including the unused tail of the last
    /// page. The footer must already have been read.
    pub fn fetch_index_page(&self, key: &str, footer: &IndexFooter, page: u32) -> Result<Vec<u8>> {
        if page >= footer.page_count {
            return Err(Error::BadResponse(format!(
                "index page {page} out of range 0..{}",
                footer.page_count
            )));
        }
        self.fetch_data_range(
            key,
            Some(".index"),
            footer.page_offset(page),
            u64::from(footer.page_size),
        )
    }

    /// Try each host in order, as the reference does; a failure of any kind
    /// moves to the next host, and only when every host failed does the last
    /// error surface.
    fn fetch_over_hosts(
        &self,
        route: &str,
        key: &str,
        suffix: Option<&str>,
        range: Option<(u64, u64)>,
    ) -> Result<Vec<u8>> {
        if key.len() != 32 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error::BadKey(key.to_string()));
        }
        let mut last = None;
        for host in &self.hosts {
            let url = build_url(host, &self.cdn_path, route, key, suffix);
            match self.get(&url, range) {
                Ok(bytes) => {
                    eprintln!("cdn: fetched {url} ({} B)", bytes.len());
                    return Ok(bytes);
                }
                Err(e) => {
                    eprintln!("cdn: {url}: {e}");
                    last = Some(e);
                }
            }
        }
        Err(last.unwrap_or(Error::NoHosts))
    }

    /// One GET, following up to [`MAX_REDIRECTS`] http redirects.
    fn get(&self, url: &str, range: Option<(u64, u64)>) -> Result<Vec<u8>> {
        let mut current = url.to_string();
        for _ in 0..=MAX_REDIRECTS {
            let parsed = parse_http_url(&current)?;
            let mut stream = self.connect(&parsed)?;
            let body = request(&mut stream, &parsed, range, self.timeout)?;
            let response = parse_response(&body)?;
            match response.status {
                200 => {
                    if let Some((offset, end)) = range {
                        // A server that ignored the Range header: keep only
                        // the asked-for window when the body covers it.
                        let need = (end + 1) as usize;
                        if body_covers(&response.body, need, offset) {
                            return Ok(response.body[offset as usize..need].to_vec());
                        }
                        return Err(Error::BadResponse(format!(
                            "server ignored Range and returned {} B for bytes {offset}-{end}",
                            response.body.len()
                        )));
                    }
                    return Ok(response.body);
                }
                206 => {
                    if range.is_none() {
                        return Err(Error::BadResponse(
                            "got 206 without asking for a range".to_string(),
                        ));
                    }
                    return Ok(response.body);
                }
                301 | 302 | 303 | 307 | 308 => {
                    let Some(location) = response
                        .headers
                        .iter()
                        .find(|(n, _)| n == "location")
                        .map(|(_, v)| v.clone())
                    else {
                        return Err(Error::BadResponse(format!(
                            "{} redirect without a Location",
                            response.status
                        )));
                    };
                    current = resolve_redirect(&parsed, &location);
                    continue;
                }
                404 => return Err(Error::NotFound(current)),
                code => return Err(Error::Status(code, current)),
            }
        }
        Err(Error::TooManyRedirects(url.to_string()))
    }

    fn connect(&self, url: &ParsedUrl) -> Result<TcpStream> {
        let timeout = self.timeout;
        let addrs = (url.host.as_str(), url.port)
            .to_socket_addrs()
            .map_err(|e| Error::BadUrl(format!("{}: {e}", url.host)))?;
        for addr in addrs {
            match TcpStream::connect_timeout(&addr, timeout) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(timeout))?;
                    stream.set_write_timeout(Some(timeout))?;
                    return Ok(stream);
                }
                Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
                    return Err(Error::Io(e));
                }
                Err(_) => continue,
            }
        }
        Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            format!("could not connect to {}", url.host),
        )))
    }
}

/// Normalize one `CDN Hosts` entry: bare host gets `http://`, and a URL this
/// client cannot speak (https) is refused up front.
fn normalize_host(host: &str) -> Result<String> {
    let host = host.trim().trim_end_matches('/');
    if host.is_empty() {
        return Err(Error::BadUrl("empty CDN host".to_string()));
    }
    let bytes = host.as_bytes();
    // Byte-slice, not `&str`-slice: a multi-byte character straddling the
    // boundary would panic the slicing.
    if bytes.len() >= 8 && bytes[..8].eq_ignore_ascii_case(b"https://") {
        return Err(Error::TlsRequired(host.to_string()));
    }
    if bytes.len() >= 7 && bytes[..7].eq_ignore_ascii_case(b"http://") {
        return Ok(host.to_string());
    }
    Ok(format!("http://{host}"))
}

/// A key must be 32 lowercase-or-uppercase hex chars (16 bytes) before it is
/// used to build a cache path or a URL: anything else is rejected with
/// [`Error::BadKey`]. Validating first avoids slicing `&key[0..2]` on a short
/// key (panic) and keeps non-hex input out of filesystem paths.
fn validate_key(key: &str) -> Result<()> {
    if key.len() != 32 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::BadKey(key.to_string()));
    }
    Ok(())
}

/// `{base}/{cdn_path}/{route}/{k0k1}/{k2k3}/{key}{suffix}` — the reference's
/// `HttpCDNClient.Fetch` URL shape.
fn build_url(base: &str, cdn_path: &str, route: &str, key: &str, suffix: Option<&str>) -> String {
    let suffix = suffix.unwrap_or("");
    if cdn_path.is_empty() {
        format!("{base}/{route}/{}/{}/{key}{suffix}", &key[0..2], &key[2..4])
    } else {
        format!(
            "{base}/{cdn_path}/{route}/{}/{}/{key}{suffix}",
            &key[0..2],
            &key[2..4]
        )
    }
}

#[derive(Debug, Clone)]
struct ParsedUrl {
    host: String,
    port: u16,
    path: String,
}

fn parse_http_url(url: &str) -> Result<ParsedUrl> {
    let rest = if let Some(rest) = url.strip_prefix("http://") {
        rest
    } else if url.starts_with("https://") {
        return Err(Error::TlsRequired(url.to_string()));
    } else {
        return Err(Error::BadUrl(url.to_string()));
    };
    let (authority, path) = match rest.find('/') {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()) => (
            h.to_string(),
            p.parse::<u16>()
                .map_err(|_| Error::BadUrl(url.to_string()))?,
        ),
        _ => (authority.to_string(), 80),
    };
    if host.is_empty() {
        return Err(Error::BadUrl(url.to_string()));
    }
    Ok(ParsedUrl {
        host,
        port,
        path: path.to_string(),
    })
}

/// Send the GET and read the whole response (headers and body) back.
fn request(
    stream: &mut TcpStream,
    url: &ParsedUrl,
    range: Option<(u64, u64)>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let mut head = format!(
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\nUser-Agent: ow-casc/0.1\r\n",
        url.path, url.host
    );
    if let Some((start, end)) = range {
        head.push_str(&format!("Range: bytes={start}-{end}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.flush()?;

    let deadline = std::time::Instant::now() + timeout;
    let mut raw = Vec::with_capacity(64 * 1024);
    let mut buf = [0u8; 64 * 1024];
    loop {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() as u64 > MAX_BODY {
                    return Err(Error::BadResponse(format!(
                        "response exceeds {MAX_BODY} bytes"
                    )));
                }
            }
            Err(e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                if std::time::Instant::now() >= deadline {
                    return Err(Error::Io(e));
                }
            }
            Err(e) => return Err(Error::Io(e)),
        }
    }
    Ok(raw)
}

struct Response {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn head_end(raw: &[u8]) -> Option<usize> {
    raw.windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|at| at + 4)
}

fn parse_response(raw: &[u8]) -> Result<Response> {
    let Some(end) = head_end(raw) else {
        return Err(Error::BadResponse("no header terminator".to_string()));
    };
    let head = String::from_utf8_lossy(&raw[..end]);
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or("");
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| Error::BadResponse(format!("bad status line {status_line:?}")))?;
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let body_start = end;
    let mut body = raw[body_start..].to_vec();

    let chunked = headers
        .iter()
        .any(|(n, v)| n == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked"));
    if chunked {
        body = decode_chunked(&body)?;
    } else {
        let content_len = headers
            .iter()
            .find(|(n, _)| n == "content-length")
            .map(|(_, v)| v);
        if status == 200 || status == 206 {
            // Non-chunked 200/206 must frame the body: a missing or
            // unparsable Content-Length, or fewer bytes than it promises,
            // is a truncated response, not content. Range fetches depend on
            // this to notice a short read.
            let Some(v) = content_len else {
                return Err(Error::BadResponse(
                    "missing Content-Length on non-chunked 200/206".to_string(),
                ));
            };
            let len: usize = v
                .parse()
                .map_err(|_| Error::BadResponse(format!("bad Content-Length {v:?}")))?;
            if body.len() > len {
                body.truncate(len);
            }
            if body.len() < len {
                return Err(Error::BadResponse(format!(
                    "body is {} B, Content-Length says {len}",
                    body.len()
                )));
            }
        } else if let Some(v) = content_len {
            if let Ok(len) = v.parse::<usize>() {
                if body.len() > len {
                    body.truncate(len);
                }
            }
        }
    }
    Ok(Response {
        status,
        headers,
        body,
    })
}

fn decode_chunked(raw: &[u8]) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let Some(line_end) = raw
            .get(at..)
            .and_then(|slice| slice.windows(2).position(|w| w == b"\r\n").map(|p| at + p))
        else {
            return Err(Error::BadResponse(
                "chunked body without a size line".to_string(),
            ));
        };
        let size_text = String::from_utf8_lossy(&raw[at..line_end]);
        let size =
            usize::from_str_radix(size_text.trim().split(';').next().unwrap_or("").trim(), 16)
                .map_err(|_| Error::BadResponse(format!("bad chunk size {size_text:?}")))?;
        at = line_end + 2;
        if size == 0 {
            return Ok(out);
        }
        // `checked_add` before the slice: a hostile size line must be a bad
        // response, not an overflow panic (or a wrapped slice in release).
        let Some(end) = at.checked_add(size) else {
            return Err(Error::BadResponse("chunk size overflows".to_string()));
        };
        if end > raw.len() {
            return Err(Error::BadResponse("chunk runs past the body".to_string()));
        }
        out.extend_from_slice(&raw[at..end]);
        if raw.get(end..end + 2) != Some(b"\r\n") {
            return Err(Error::BadResponse("chunk not terminated".to_string()));
        }
        at = end + 2;
    }
}

fn body_covers(body: &[u8], need: usize, offset: u64) -> bool {
    body.len() >= need && body.len() > offset as usize
}

fn resolve_redirect(from: &ParsedUrl, location: &str) -> String {
    if location.starts_with("http://") || location.starts_with("https://") {
        return location.to_string();
    }
    let base = format!("http://{}", from.host);
    let base = if from.port == 80 {
        base
    } else {
        format!("{}:{}", base, from.port)
    };
    if location.starts_with('/') {
        return format!("{base}{location}");
    }
    let dir = match from.path.rfind('/') {
        Some(at) => &from.path[..=at],
        None => "/",
    };
    format!("{base}{dir}{location}")
}

/// Write cache files via a temp file and a rename, so a killed process never
/// leaves a half-written file that a later run would read as content.
fn store_cache(path: &Path, bytes: &[u8]) {
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = path.with_extension("tmp");
    if fs::write(&tmp, bytes).is_err() {
        let _ = fs::remove_file(&tmp);
        return;
    }
    if fs::rename(&tmp, path).is_err() {
        let _ = fs::remove_file(path);
        let _ = fs::rename(&tmp, path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::sync::Arc;
    use std::sync::Mutex;

    fn client(hosts: Vec<String>, cache: Option<PathBuf>) -> CdnClient {
        CdnClient::new(&hosts, "tpr/ovw", cache, Duration::from_secs(5)).expect("client")
    }

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("ow-cdn-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    struct Mock {
        port: u16,
        requests: Arc<Mutex<Vec<String>>>,
        handle: Option<std::thread::JoinHandle<()>>,
    }

    impl Mock {
        /// Serve `responses` in order, capturing each request's first line and
        /// headers. A response of `None` means "close without answering".
        fn start(name: &str, responses: Vec<Option<Vec<u8>>>) -> Mock {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = listener.local_addr().expect("addr").port();
            let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
            let captured = Arc::clone(&requests);
            let name = name.to_string();
            let handle = std::thread::spawn(move || {
                for response in responses {
                    let Ok((mut stream, _)) = listener.accept() else {
                        break;
                    };
                    let mut reader = std::io::BufReader::new(stream.try_clone().expect("clone"));
                    let mut text = String::new();
                    loop {
                        let mut line = String::new();
                        if reader.read_line(&mut line).unwrap_or(0) == 0 {
                            break;
                        }
                        text.push_str(&line);
                        if line == "\r\n" {
                            break;
                        }
                    }
                    captured
                        .lock()
                        .expect("lock")
                        .push(format!("{name}:{text}"));
                    if let Some(response) = response {
                        let _ = stream.write_all(&response);
                    }
                }
            });
            Mock {
                port,
                requests,
                handle: Some(handle),
            }
        }

        fn url(&self) -> String {
            format!("http://127.0.0.1:{}", self.port)
        }
    }

    impl Drop for Mock {
        fn drop(&mut self) {
            if let Some(handle) = self.handle.take() {
                let _ = handle.join();
            }
        }
    }

    fn ok_response(body: &[u8]) -> Vec<u8> {
        let mut out =
            format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n", body.len()).into_bytes();
        out.extend_from_slice(body);
        out
    }

    fn partial_response(body: &[u8]) -> Vec<u8> {
        let mut out = format!(
            "HTTP/1.1 206 Partial Content\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        out.extend_from_slice(body);
        out
    }

    /// A synthetic loose index with the client's layout and `n` entries.
    /// 1024-byte pages, 20-byte loose entries, 8-byte checksums.
    fn build_index(n: usize) -> Vec<u8> {
        const PAGE: usize = 1024;
        const CK: usize = 8;
        let epp = PAGE / 20;
        let pages = ((n + epp - 1).max(1) / epp).max(1);
        let mut out = Vec::new();
        for p in 0..pages {
            let mut page = vec![0u8; PAGE];
            for i in 0..epp {
                let index = p * epp + i;
                if index >= n {
                    break;
                }
                let at = i * 20;
                page[at..at + 16].copy_from_slice(&[index as u8; 16]);
                page[at + 16..at + 20].copy_from_slice(&(index as u32 + 7).to_be_bytes());
            }
            out.extend_from_slice(&page);
        }
        let page_end = out.len();
        for p in 0..pages {
            let last = (p * epp + epp - 1).min(n - 1) as u8;
            out.extend_from_slice(&[last; 16]);
        }
        let keys = out[page_end..].to_vec();
        let mut checksums = Vec::new();
        for page in out[..page_end].chunks_exact(PAGE) {
            checksums.extend_from_slice(&crate::blte::md5_digest(page)[..CK]);
        }
        out.extend_from_slice(&checksums);
        let mut tail = keys;
        tail.extend_from_slice(&checksums);
        out.extend_from_slice(&crate::blte::md5_digest(&tail)[..CK]);
        let mut footer = [0u8; 20];
        footer[0] = 1;
        footer[3] = (PAGE / 1024) as u8;
        footer[4] = 0;
        footer[5] = 4;
        footer[6] = 16;
        footer[7] = CK as u8;
        footer[8..12].copy_from_slice(&(n as u32).to_le_bytes());
        let mut header = footer;
        header[12..20].fill(0);
        let hash = crate::blte::md5_digest(&header);
        footer[12..20].copy_from_slice(&hash[..8]);
        out.extend_from_slice(&footer);
        out
    }

    fn text_response(text: &str) -> Vec<u8> {
        text.as_bytes().to_vec()
    }

    #[test]
    fn url_shape_matches_the_reference() {
        let url = build_url(
            "http://level3.blizzard.com",
            "tpr/ovw",
            "data",
            "5c98ebc39c4ced65486935cb85968b58",
            Some(".index"),
        );
        assert_eq!(
            url,
            "http://level3.blizzard.com/tpr/ovw/data/5c/98/\
             5c98ebc39c4ced65486935cb85968b58.index"
        );
    }

    #[test]
    fn bare_hosts_get_http_and_https_is_refused() {
        assert_eq!(
            normalize_host("level3.blizzard.com").unwrap(),
            "http://level3.blizzard.com"
        );
        assert!(matches!(
            normalize_host("https://level3.blizzard.com"),
            Err(Error::TlsRequired(_))
        ));
    }

    #[test]
    fn parses_plain_and_chunked_bodies() {
        let plain = parse_response(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello").unwrap();
        assert_eq!(plain.status, 200);
        assert_eq!(plain.body, b"hello");

        let chunked = parse_response(
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        )
        .unwrap();
        assert_eq!(chunked.body, b"hello");

        assert!(parse_response(b"garbage").is_err());
    }

    #[test]
    fn fetch_and_cache_write_through() {
        let cache = temp_dir("write-through");
        let body = b"BLTE-not-really";
        let mock = Mock::start("write-through", vec![Some(ok_response(body))]);
        let client = client(vec![mock.url()], Some(cache.clone()));

        let key = "169fff78cb3259595e68cd8ca6eca5bb";
        let first = client.fetch_data(key, None).expect("first fetch");
        assert_eq!(first, body);
        let cached = cache.join("data").join("16").join("9f").join(key);
        assert!(cached.is_file(), "cache file written");

        let second = client.fetch_data(key, None).expect("second fetch");
        assert_eq!(second, body);
        let requests = mock.requests.lock().unwrap().len();
        assert_eq!(requests, 1, "second call must not hit the server");
        let _ = fs::remove_dir_all(&cache);
    }

    #[test]
    fn range_request_carries_the_header_and_takes_206() {
        let mut response = b"HTTP/1.1 206 Partial Content\r\nContent-Length: 100\r\n\r\n".to_vec();
        response.extend_from_slice(&[b'r'; 100]);
        let mock = Mock::start("range", vec![Some(response)]);
        let client = client(vec![mock.url()], None);
        let key = "2f46d225587a744e40ec2f5fe1c3b99e";
        let bytes = client.fetch_range(key, 4096, 100).expect("range fetch");
        assert_eq!(bytes.len(), 100);
        let requests = mock.requests.lock().unwrap();
        let request = &requests[0];
        assert!(
            request.contains("Range: bytes=4096-4195\r\n"),
            "request did not carry the range: {request:?}"
        );
        assert!(request.contains("/tpr/ovw/data/2f/46/2f46d225587a744e40ec2f5fe1c3b99e "));
    }

    #[test]
    fn a_200_for_a_range_is_sliced_from_the_full_body() {
        let full: Vec<u8> = (0..=255u8).collect();
        let mock = Mock::start("slice", vec![Some(ok_response(&full))]);
        let client = client(vec![mock.url()], None);
        let bytes = client
            .fetch_range("2f46d225587a744e40ec2f5fe1c3b99e", 10, 5)
            .expect("sliced fetch");
        assert_eq!(bytes, &full[10..15]);
    }

    #[test]
    fn missing_file_reports_not_found() {
        let mock = Mock::start(
            "404",
            vec![Some(text_response(
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
            ))],
        );
        let client = client(vec![mock.url()], None);
        let err = client
            .fetch_data("5c98ebc39c4ced65486935cb85968b58", None)
            .unwrap_err();
        assert!(matches!(err, Error::NotFound(_)), "got {err:?}");
    }

    #[test]
    fn dead_first_host_falls_through_to_the_second() {
        // Bind and drop a listener so the port is (almost surely) dead.
        let dead_port = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("addr").port()
        };
        let body = b"from host two";
        let mock = Mock::start("fallback", vec![Some(ok_response(body))]);
        let client = client(
            vec![format!("http://127.0.0.1:{dead_port}"), mock.url()],
            None,
        );
        let bytes = client
            .fetch_data("169fff78cb3259595e68cd8ca6eca5bb", None)
            .expect("second host answers");
        assert_eq!(bytes, body);
    }

    #[test]
    fn https_redirect_is_refused() {
        let mock = Mock::start(
            "redirect",
            vec![Some(text_response(
                "HTTP/1.1 302 Found\r\nLocation: https://cdn.example/secret\r\n\
                 Content-Length: 0\r\n\r\n",
            ))],
        );
        let client = client(vec![mock.url()], None);
        let err = client
            .fetch_data("169fff78cb3259595e68cd8ca6eca5bb", None)
            .unwrap_err();
        assert!(matches!(err, Error::TlsRequired(_)), "got {err:?}");
    }

    #[test]
    fn http_redirect_is_followed() {
        let mock = Mock::start(
            "hop",
            vec![
                Some(text_response(
                    "HTTP/1.1 302 Found\r\nLocation: /tpr/ovw/data/16/9f/\
                     169fff78cb3259595e68cd8ca6eca5bb\r\nContent-Length: 0\r\n\r\n",
                )),
                Some(ok_response(b"after redirect")),
            ],
        );
        let client = client(vec![mock.url()], None);
        let bytes = client
            .fetch_data("169fff78cb3259595e68cd8ca6eca5bb", None)
            .expect("followed");
        assert_eq!(bytes, b"after redirect");
    }

    #[test]
    fn bad_keys_are_refused_before_any_socket() {
        let client = client(vec!["http://127.0.0.1:1".to_string()], None);
        assert!(matches!(
            client.fetch_data("not-a-key", None),
            Err(Error::BadKey(_))
        ));
    }

    #[test]
    fn config_route_is_distinct_from_data_route() {
        let cache = temp_dir("config-route");
        let mock = Mock::start("config", vec![Some(ok_response(b"root = f883c998\n"))]);
        let client = client(vec![mock.url()], Some(cache.clone()));
        let bytes = client
            .fetch_config("c3fcf2a11f2638cfdb16c6554e389b72")
            .expect("config");
        assert_eq!(bytes, b"root = f883c998\n");
        let request = mock.requests.lock().unwrap()[0].clone();
        assert!(
            request.contains("GET /tpr/ovw/config/c3/fc/c3fcf2a11f2638cfdb16c6554e389b72 "),
            "request path: {request:?}"
        );
        assert!(cache
            .join("config")
            .join("c3")
            .join("fc")
            .join("c3fcf2a11f2638cfdb16c6554e389b72")
            .is_file());
        let _ = fs::remove_dir_all(&cache);
    }

    /// The footer request is the CPid's mode-1 path and the last 20 bytes:
    /// `Range: bytes=len-20-len-1` on `data/…/<key>.index`.
    #[test]
    fn index_footer_range_is_the_last_twenty_bytes() {
        let index = build_index(3);
        let key = "c3fcf2a11f2638cfdb16c6554e389b72";
        let footer = index[index.len() - 20..].to_vec();
        let mock = Mock::start("index-footer", vec![Some(partial_response(&footer))]);
        let client = client(vec![mock.url()], None);
        let bytes = client
            .fetch_index_footer(key, index.len() as u64)
            .expect("footer");
        assert_eq!(bytes.as_slice(), footer.as_slice());
        let parsed = IndexFooter::parse(&bytes, index.len() as u32).expect("footer parse");
        assert_eq!(parsed.page_count, 1);
        assert_eq!(parsed.element_count, 3);

        let requests = mock.requests.lock().unwrap();
        let request = &requests[0];
        assert!(
            request.contains(&format!("GET /tpr/ovw/data/c3/fc/{key}.index HTTP/1.1")),
            "path: {request:?}"
        );
        assert!(
            request.contains(&format!(
                "Range: bytes={}-{}\r\n",
                index.len() - 20,
                index.len() - 1
            )),
            "range: {request:?}"
        );
    }

    /// The tail and page ranges are the client's layout spans, and the page
    /// decodes through the same footer geometry.
    #[test]
    fn index_tail_and_page_ranges_follow_the_footer() {
        let index = build_index(52);
        let key = "2f46d225587a744e40ec2f5fe1c3b99e";
        let file_len = index.len();
        let footer_bytes: [u8; 20] = index[file_len - 20..].try_into().unwrap();
        let footer = IndexFooter::parse(&footer_bytes, file_len as u32).expect("footer");
        assert_eq!(footer.page_count, 2);
        assert_eq!(footer.entries_per_page, 51);
        assert_eq!(footer.tail_len(), 2 * 16 + 2 * 8 + 8);

        let tail_at = (footer.page_count * footer.page_size) as usize;
        let tail_len = footer.tail_len() as usize;
        let tail = index[tail_at..tail_at + tail_len].to_vec();
        let page1_at = footer.page_offset(1) as usize;
        let page1 = index[page1_at..page1_at + footer.page_size as usize].to_vec();

        let mock = Mock::start(
            "index-spans",
            vec![
                Some(partial_response(&footer_bytes)),
                Some(partial_response(&tail)),
                Some(partial_response(&page1)),
            ],
        );
        let client = client(vec![mock.url()], None);
        let got_footer = client
            .fetch_index_footer(key, file_len as u64)
            .expect("footer");
        assert_eq!(got_footer, footer_bytes);
        let got_tail = client.fetch_index_tail(key, &footer).expect("tail");
        assert_eq!(got_tail, tail);
        let got_page = client.fetch_index_page(key, &footer, 1).expect("page 1");
        assert_eq!(got_page, page1);

        let entries = footer.entries_in_page(1, &got_page).expect("decode page");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].key, [51u8; 16]);
        assert_eq!(entries[0].size, 58);
        assert_eq!(entries[0].offset, 0);
        assert_eq!(entries[0].archive, 0);

        let requests = mock.requests.lock().unwrap();
        let expected = [
            format!("Range: bytes={}-{}\r\n", file_len - 20, file_len - 1),
            format!("Range: bytes={tail_at}-{}\r\n", tail_at + tail_len - 1),
            format!(
                "Range: bytes={page1_at}-{}\r\n",
                page1_at + footer.page_size as usize - 1
            ),
        ];
        for (request, range) in requests.iter().zip(expected) {
            assert!(
                request.contains(&format!("GET /tpr/ovw/data/2f/46/{key}.index HTTP/1.1")),
                "path: {request:?}"
            );
            assert!(request.contains(&range), "want {range:?} in {request:?}");
        }

        assert!(matches!(
            client.fetch_index_page(key, &footer, 2),
            Err(Error::BadResponse(_))
        ));
        assert!(matches!(
            client.fetch_index_footer(key, 19),
            Err(Error::BadResponse(_))
        ));
    }
}
