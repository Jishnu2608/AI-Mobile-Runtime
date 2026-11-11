//! INS-001: HTTPS downloads that resume after interruptions.
//!
//! - Only `https://` URLs on the [`Policy`] host allowlist; every redirect is checked too.
//! - Partial data is kept as `<file>.part`; the next attempt asks for the rest with
//!   `Range: bytes=<n>-`. A server that ignores the range (200) restarts the file; 416 means
//!   the part is already complete.
//! - Retries with backoff on network errors; cancel is checked between chunks.
//! - Proxy: `HTTPS_PROXY` / `HTTP_PROXY` / `NO_PROXY` and the Windows system proxy are used
//!   (reqwest defaults). TLS uses the Windows certificate store (`SChannel`).
//! - The file only gets its final name after the checksum matched (INS-002).

use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{CONTENT_RANGE, RANGE, USER_AGENT};
use reqwest::{StatusCode, Url};

use crate::verify::{self, Checksum};
use crate::InstallError;

/// Which hosts may be contacted.
#[derive(Debug, Clone)]
pub struct Policy {
    /// Exact host names, or suffixes starting with `.` (e.g. `.githubusercontent.com`).
    pub hosts: Vec<String>,
    /// Tests only: allow `http://127.0.0.1` / `localhost`.
    pub allow_loopback_http: bool,
    pub retries: u32,
    pub backoff: Duration,
}

impl Policy {
    /// Official sources (master build §11.5): Adoptium, nodejs.org, Google SDK repository,
    /// Flutter storage, and the GitHub release CDN Adoptium redirects to.
    #[must_use]
    pub fn official() -> Self {
        Self {
            hosts: [
                "api.adoptium.net",
                "github.com",
                ".githubusercontent.com",
                "nodejs.org",
                "dl.google.com",
                "dl-ssl.google.com",
                ".gvt1.com",
                "storage.googleapis.com",
            ]
            .map(String::from)
            .to_vec(),
            allow_loopback_http: false,
            retries: 5,
            backoff: Duration::from_secs(2),
        }
    }

    #[must_use]
    pub fn allows(&self, url: &Url) -> bool {
        let Some(host) = url.host_str() else {
            return false;
        };
        let host = host.to_ascii_lowercase();
        if url.scheme() == "http" {
            return self.allow_loopback_http && (host == "127.0.0.1" || host == "localhost");
        }
        if url.scheme() != "https" {
            return false;
        }
        self.hosts.iter().any(|h| {
            if let Some(suffix) = h.strip_prefix('.') {
                host.ends_with(&format!(".{suffix}"))
            } else {
                host == *h
            }
        })
    }
}

/// Download progress callback: (bytes so far, total if known).
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>);

#[derive(Debug)]
pub struct Http {
    client: Client,
    policy: Policy,
}

fn user_agent() -> String {
    format!("umr-installer/{}", env!("CARGO_PKG_VERSION"))
}

impl Http {
    pub fn new(policy: Policy) -> Result<Self, InstallError> {
        let p = policy.clone();
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(30))
            // Blocking client: applies to sending and to each body read, so a stalled transfer
            // fails after 60 s while long downloads (system images: many minutes) still work.
            .timeout(Duration::from_secs(60))
            .redirect(reqwest::redirect::Policy::custom(move |attempt| {
                if attempt.previous().len() > 10 {
                    attempt.error("too many redirects")
                } else if p.allows(attempt.url()) {
                    attempt.follow()
                } else {
                    let u = attempt.url().to_string();
                    attempt.error(format!("redirect to a host that is not allowed: {u}"))
                }
            }))
            .build()
            .map_err(|e| InstallError::Network(e.to_string()))?;
        Ok(Self { client, policy })
    }

    fn checked_url(&self, url: &str) -> Result<Url, InstallError> {
        let u = Url::parse(url).map_err(|_| InstallError::HostNotAllowed(url.into()))?;
        if self.policy.allows(&u) {
            Ok(u)
        } else {
            Err(InstallError::HostNotAllowed(url.into()))
        }
    }

    fn retrying<T>(
        &self,
        cancel: &AtomicBool,
        mut f: impl FnMut() -> Result<T, InstallError>,
    ) -> Result<T, InstallError> {
        let mut attempt = 0;
        loop {
            match f() {
                Err(InstallError::Network(e)) if attempt < self.policy.retries => {
                    attempt += 1;
                    let wait = self.policy.backoff * (1 << (attempt - 1).min(4));
                    let until = std::time::Instant::now() + wait;
                    while std::time::Instant::now() < until {
                        if cancel.load(Ordering::Relaxed) {
                            return Err(InstallError::Cancelled);
                        }
                        std::thread::sleep(Duration::from_millis(50).min(wait));
                    }
                    let _ = e;
                }
                other => return other,
            }
        }
    }

    /// Small text document (metadata), at most `max_bytes`.
    pub fn get_text(&self, url: &str, max_bytes: u64) -> Result<String, InstallError> {
        let u = self.checked_url(url)?;
        self.retrying(&AtomicBool::new(false), || {
            let resp = self
                .client
                .get(u.clone())
                .header(USER_AGENT, user_agent())
                .send()
                .map_err(net)?;
            if !resp.status().is_success() {
                return Err(InstallError::Http {
                    status: resp.status().as_u16(),
                    url: url.into(),
                });
            }
            let mut body = Vec::new();
            resp.take(max_bytes + 1)
                .read_to_end(&mut body)
                .map_err(|e| InstallError::Network(e.to_string()))?;
            if body.len() as u64 > max_bytes {
                return Err(InstallError::Metadata(format!(
                    "{url} is larger than {max_bytes} bytes"
                )));
            }
            String::from_utf8(body).map_err(|e| InstallError::Metadata(e.to_string()))
        })
    }

    /// Downloads `url` to `dest`, resuming `dest.part`, and verifies the checksum.
    /// A `dest` that already exists with the right checksum is reused (download cache).
    pub fn download(
        &self,
        url: &str,
        dest: &Path,
        expected: &Checksum,
        size: Option<u64>,
        progress: Progress<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError> {
        let u = self.checked_url(url)?;
        if dest.is_file() && verify::check(dest, expected).is_ok() {
            let len = dest.metadata()?.len();
            progress(len, Some(len));
            return Ok(());
        }
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let part = part_path(dest);
        let mut restarted = false;
        loop {
            self.retrying(cancel, || {
                self.fetch_into(&u, &part, size, progress, cancel)
            })?;
            match verify::check(&part, expected) {
                Ok(()) => {
                    std::fs::rename(&part, dest)?;
                    return Ok(());
                }
                Err(e) => {
                    // Corrupt or stale partial data: start once more from zero, then give up.
                    let _ = std::fs::remove_file(&part);
                    if restarted {
                        return Err(e);
                    }
                    restarted = true;
                }
            }
        }
    }

    /// One attempt: appends the missing bytes to `part`.
    fn fetch_into(
        &self,
        u: &Url,
        part: &Path,
        size: Option<u64>,
        progress: Progress<'_>,
        cancel: &AtomicBool,
    ) -> Result<(), InstallError> {
        if cancel.load(Ordering::Relaxed) {
            return Err(InstallError::Cancelled);
        }
        let have = part.metadata().map_or(0, |m| m.len());
        if size.is_some_and(|s| have == s) {
            return Ok(());
        }
        let mut req = self.client.get(u.clone()).header(USER_AGENT, user_agent());
        if have > 0 {
            req = req.header(RANGE, format!("bytes={have}-"));
        }
        let mut resp = req.send().map_err(net)?;
        let status = resp.status();
        let (mut file, mut done) = match status {
            StatusCode::PARTIAL_CONTENT => {
                let starts_at = resp
                    .headers()
                    .get(CONTENT_RANGE)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.strip_prefix("bytes "))
                    .and_then(|v| v.split('-').next())
                    .and_then(|v| v.parse::<u64>().ok());
                if starts_at != Some(have) {
                    let _ = std::fs::remove_file(part);
                    return Err(InstallError::Network(
                        "server resumed at the wrong offset; restarting".into(),
                    ));
                }
                (OpenOptions::new().append(true).open(part)?, have)
            }
            StatusCode::OK => (File::create(part)?, 0),
            StatusCode::RANGE_NOT_SATISFIABLE if have > 0 => return Ok(()),
            s => {
                return Err(InstallError::Http {
                    status: s.as_u16(),
                    url: u.to_string(),
                })
            }
        };
        let total = size.or_else(|| resp.content_length().map(|l| l + done));
        progress(done, total);
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            if cancel.load(Ordering::Relaxed) {
                file.flush()?;
                return Err(InstallError::Cancelled);
            }
            let n = match resp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => {
                    file.flush()?;
                    return Err(InstallError::Network(e.to_string()));
                }
            };
            file.write_all(&buf[..n])?;
            done += n as u64;
            progress(done, total);
        }
        file.flush()?;
        if let Some(t) = total {
            if done < t {
                return Err(InstallError::Network(format!(
                    "connection closed after {done} of {t} bytes"
                )));
            }
        }
        Ok(())
    }
}

#[allow(clippy::needless_pass_by_value)]
fn net(e: reqwest::Error) -> InstallError {
    if e.is_redirect() {
        InstallError::HostNotAllowed(e.to_string())
    } else if let Some(s) = e.status() {
        InstallError::Http {
            status: s.as_u16(),
            url: e.url().map(ToString::to_string).unwrap_or_default(),
        }
    } else {
        InstallError::Network(e.to_string())
    }
}

/// `<dest>.part`
#[must_use]
pub fn part_path(dest: &Path) -> PathBuf {
    let mut p = dest.as_os_str().to_os_string();
    p.push(".part");
    PathBuf::from(p)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;
    use crate::testkit::{tmp_dir, Route, TestServer};
    use crate::verify::sha256_hex;

    fn test_policy() -> Policy {
        Policy {
            hosts: vec![],
            allow_loopback_http: true,
            retries: 3,
            backoff: Duration::from_millis(10),
        }
    }

    fn data() -> Vec<u8> {
        (0..300_000u32).map(|i| (i % 251) as u8).collect()
    }

    fn server(route: Route) -> TestServer {
        TestServer::start(HashMap::from([("/f.zip".to_string(), route)]))
    }

    #[test]
    fn policy_allows_only_official_https_hosts() {
        let p = Policy::official();
        let ok = |u: &str| p.allows(&Url::parse(u).unwrap());
        assert!(ok("https://api.adoptium.net/v3/x"));
        assert!(ok("https://objects.githubusercontent.com/a"));
        assert!(ok("https://dl.google.com/android/repository/x.zip"));
        assert!(
            !ok("http://dl.google.com/android/repository/x.zip"),
            "plain http"
        );
        assert!(!ok("https://dl.google.com.evil.example/x"));
        assert!(!ok("https://evilgithubusercontent.com/x"));
        assert!(!ok("http://127.0.0.1/x"), "loopback only in tests");
        assert!(!ok("file:///C:/x"));
    }

    #[test]
    fn downloads_and_verifies() {
        let d = data();
        let s = server(Route::body(&d));
        let dir = tmp_dir("http-ok");
        let dest = dir.join("f.zip");
        let http = Http::new(test_policy()).unwrap();
        let mut last = (0, None);
        http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256(sha256_hex(&d)),
            None,
            &mut |a, b| last = (a, b),
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), d);
        assert_eq!(last, (d.len() as u64, Some(d.len() as u64)));
        assert!(!part_path(&dest).exists());
        // cached: no new request
        let before = s.requests.load(Ordering::SeqCst);
        http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256(sha256_hex(&d)),
            None,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(s.requests.load(Ordering::SeqCst), before);
    }

    #[test]
    fn resumes_after_the_connection_drops() {
        let d = data();
        let s = server(Route {
            cut_after: Some((100_000, 2)),
            ..Route::body(&d)
        });
        let dir = tmp_dir("http-resume");
        let dest = dir.join("f.zip");
        let http = Http::new(test_policy()).unwrap();
        http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256(sha256_hex(&d)),
            Some(d.len() as u64),
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), d);
        let ranges = s.ranges.lock().unwrap().clone();
        assert_eq!(
            ranges,
            vec![
                None,
                Some("bytes=100000-".into()),
                Some("bytes=200000-".into())
            ]
        );
    }

    #[test]
    fn server_ignoring_range_restarts_the_file() {
        let d = data();
        let s = server(Route {
            ignore_range: true,
            ..Route::body(&d)
        });
        let dir = tmp_dir("http-norange");
        let dest = dir.join("f.zip");
        std::fs::write(part_path(&dest), &d[..5000]).unwrap();
        let http = Http::new(test_policy()).unwrap();
        http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256(sha256_hex(&d)),
            None,
            &mut |_, _| {},
            &AtomicBool::new(false),
        )
        .unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), d, "no duplicated prefix");
    }

    #[test]
    fn checksum_mismatch_fails_and_leaves_nothing() {
        let d = data();
        let s = server(Route::body(&d));
        let dir = tmp_dir("http-bad");
        let dest = dir.join("f.zip");
        let http = Http::new(test_policy()).unwrap();
        let r = http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256("00".repeat(32)),
            None,
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(InstallError::Checksum { .. })), "{r:?}");
        assert!(!dest.exists() && !part_path(&dest).exists());
        assert_eq!(
            s.requests.load(Ordering::SeqCst),
            2,
            "one fresh retry, then give up"
        );
    }

    #[test]
    fn refuses_disallowed_hosts_and_redirects() {
        let http = Http::new(Policy {
            allow_loopback_http: false,
            ..test_policy()
        })
        .unwrap();
        let dir = tmp_dir("http-host");
        let r = http.download(
            "http://127.0.0.1:1/x",
            &dir.join("x"),
            &Checksum::Sha1("00".into()),
            None,
            &mut |_, _| {},
            &AtomicBool::new(false),
        );
        assert!(matches!(r, Err(InstallError::HostNotAllowed(_))));
        let s = server(Route {
            redirect: Some("https://evil.example/x".into()),
            ..Route::body(b"")
        });
        let http = Http::new(test_policy()).unwrap();
        let r = http.get_text(&s.url("/f.zip"), 1000);
        assert!(matches!(r, Err(InstallError::HostNotAllowed(_))), "{r:?}");
    }

    #[test]
    fn cancel_keeps_the_partial_file() {
        let d = data();
        let s = server(Route::body(&d));
        let dir = tmp_dir("http-cancel");
        let dest = dir.join("f.zip");
        let http = Http::new(test_policy()).unwrap();
        let cancel = AtomicBool::new(false);
        let r = http.download(
            &s.url("/f.zip"),
            &dest,
            &Checksum::Sha256(sha256_hex(&d)),
            None,
            &mut |n, _| {
                if n > 0 {
                    cancel.store(true, Ordering::Relaxed);
                }
            },
            &cancel,
        );
        assert_eq!(r, Err(InstallError::Cancelled));
        assert!(part_path(&dest).exists() && !dest.exists());
    }

    #[test]
    fn http_errors_are_reported() {
        let s = server(Route::body(b"x"));
        let http = Http::new(test_policy()).unwrap();
        assert!(matches!(
            http.get_text(&s.url("/missing"), 100),
            Err(InstallError::Http { status: 404, .. })
        ));
        assert_eq!(http.get_text(&s.url("/f.zip"), 100).unwrap(), "x");
        assert!(matches!(
            http.get_text(&s.url("/f.zip"), 0),
            Err(InstallError::Metadata(_))
        ));
    }
}
