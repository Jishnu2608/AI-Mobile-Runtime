//! Test helpers: temp folders, zip builder and a tiny local HTTP server with Range support.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

pub fn tmp_dir(tag: &str) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!(
        "umr-inst-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

pub fn make_zip(path: &Path, files: &[(&str, &[u8])]) -> PathBuf {
    let f = std::fs::File::create(path).unwrap();
    let mut z = zip::ZipWriter::new(f);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        z.start_file(*name, opts).unwrap();
        z.write_all(data).unwrap();
    }
    z.finish().unwrap();
    path.to_path_buf()
}

/// How the server answers one path.
#[derive(Clone)]
pub struct Route {
    pub body: Vec<u8>,
    /// Close the connection after this many body bytes, for the first N requests.
    pub cut_after: Option<(usize, usize)>,
    /// Answer 200 with the full body even when a Range is asked for.
    pub ignore_range: bool,
    /// Respond with a redirect to this location instead.
    pub redirect: Option<String>,
}

impl Route {
    pub fn body(b: &[u8]) -> Self {
        Self {
            body: b.to_vec(),
            cut_after: None,
            ignore_range: false,
            redirect: None,
        }
    }
}

pub struct TestServer {
    pub base: String,
    pub requests: Arc<AtomicUsize>,
    pub ranges: Arc<Mutex<Vec<Option<String>>>>,
}

impl TestServer {
    pub fn start(routes: HashMap<String, Route>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let requests = Arc::new(AtomicUsize::new(0));
        let ranges = Arc::new(Mutex::new(Vec::new()));
        let routes = Arc::new(Mutex::new(routes));
        let (rq, rg) = (requests.clone(), ranges.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let (routes, rq, rg) = (routes.clone(), rq.clone(), rg.clone());
                std::thread::spawn(move || {
                    let _ = handle(stream, &routes, &rq, &rg);
                });
            }
        });
        Self {
            base,
            requests,
            ranges,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }
}

fn handle(
    mut s: TcpStream,
    routes: &Mutex<HashMap<String, Route>>,
    requests: &AtomicUsize,
    ranges: &Mutex<Vec<Option<String>>>,
) -> std::io::Result<()> {
    let mut r = BufReader::new(s.try_clone()?);
    let mut line = String::new();
    r.read_line(&mut line)?;
    let path = line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let mut range: Option<String> = None;
    loop {
        let mut h = String::new();
        r.read_line(&mut h)?;
        let h = h.trim_end();
        if h.is_empty() {
            break;
        }
        if let Some((k, v)) = h.split_once(':') {
            if k.eq_ignore_ascii_case("range") {
                range = Some(v.trim().to_string());
            }
        }
    }
    let n = requests.fetch_add(1, Ordering::SeqCst);
    ranges.lock().unwrap().push(range.clone());
    let route = {
        let mut map = routes.lock().unwrap();
        let Some(route) = map.get_mut(&path) else {
            s.write_all(
                b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
            )?;
            return Ok(());
        };
        let snapshot = route.clone();
        if let Some((bytes, times)) = route.cut_after {
            route.cut_after = (times > 1).then_some((bytes, times - 1));
        }
        snapshot
    };
    let _ = n;
    if let Some(loc) = &route.redirect {
        write!(s, "HTTP/1.1 302 Found\r\nLocation: {loc}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let total = route.body.len();
    let start = match (&range, route.ignore_range) {
        (Some(r), false) => r
            .trim_start_matches("bytes=")
            .trim_end_matches('-')
            .parse::<usize>()
            .unwrap_or(0),
        _ => 0,
    };
    if start >= total && total > 0 && range.is_some() && !route.ignore_range {
        write!(s, "HTTP/1.1 416 Range Not Satisfiable\r\nContent-Range: bytes */{total}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")?;
        return Ok(());
    }
    let body = &route.body[start..];
    if start > 0 {
        write!(
            s,
            "HTTP/1.1 206 Partial Content\r\nContent-Range: bytes {start}-{}/{total}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            total - 1,
            body.len()
        )?;
    } else {
        write!(
            s,
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        )?;
    }
    match route.cut_after {
        Some((bytes, _)) if bytes < body.len() => {
            s.write_all(&body[..bytes])?;
            s.flush()?;
            // drop the connection mid-body
        }
        _ => s.write_all(body)?,
    }
    Ok(())
}
