//! PRV-008 / PRV-010: the preview server.
//!
//! One server per Core (127.0.0.1, random port). The Core tells it which emulator to show
//! ([`PreviewServer::set_target`]); every connected window follows device switches. Each window
//! gets frames at its own canvas size (the emulator scales them) and sends input back.

use std::collections::HashMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tokio_tungstenite::tungstenite::{http, Message};
use umr_device::grpc::Emulator;

use crate::protocol::{self, ClientMsg, Hello};
use crate::stats::{Report, Stats};

/// The emulator a preview shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub serial: String,
    pub grpc_port: u16,
    pub grpc_token: Option<String>,
    /// Screen size in device pixels.
    pub width: u32,
    pub height: u32,
    /// AUD-P1-42/43: the part of the screen shown (x, y, width, height), when a foldable is
    /// closed on its cover screen; touches are offset into it.
    pub view: Option<(u32, u32, u32, u32)>,
}

/// How long a handed-out token stays valid before it is used.
pub const TOKEN_TTL: Duration = Duration::from_secs(300);

/// Browser origins allowed to connect (the desktop app; Vite while developing the UI).
pub const ORIGINS: &[&str] = &[
    "http://tauri.localhost",
    "https://tauri.localhost",
    "tauri://localhost",
    "http://127.0.0.1:27300",
    "http://localhost:27300",
];

#[derive(Debug)]
struct Inner {
    /// token → (may send input, expires)
    tokens: Mutex<HashMap<String, (bool, Instant)>>,
    target: watch::Sender<Option<Target>>,
    stats: Mutex<Stats>,
    clients: AtomicUsize,
}

/// The preview server. Cloning shares it.
#[derive(Debug, Clone)]
pub struct PreviewServer {
    port: u16,
    inner: Arc<Inner>,
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_micros()).unwrap_or(0))
}

fn us(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

/// 32 hex characters from the OS random source.
fn token() -> String {
    let mut b = [0u8; 16];
    let _ = getrandom::fill(&mut b);
    b.iter().fold(String::with_capacity(32), |mut s, x| {
        use std::fmt::Write as _;
        let _ = write!(s, "{x:02x}");
        s
    })
}

/// Query value of `key` in `a=1&b=2`.
fn query<'a>(q: &'a str, key: &str) -> Option<&'a str> {
    q.split('&')
        .find_map(|kv| kv.strip_prefix(key)?.strip_prefix('='))
}

/// Whether a browser origin may connect (`None`: not a browser, e.g. a test client). VS Code
/// web views (the extension's Mobile Preview panel, VSC-001) have a `vscode-webview://<id>`
/// origin that no web page can claim.
#[must_use]
pub fn origin_allowed(origin: Option<&str>) -> bool {
    origin.is_none_or(|o| {
        ORIGINS.contains(&o)
            || o.strip_prefix("vscode-webview://").is_some_and(|id| {
                !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
            })
    })
}

impl PreviewServer {
    /// Starts listening on 127.0.0.1 (random port) on the current Tokio runtime.
    pub async fn start() -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await?;
        let port = listener.local_addr()?.port();
        let (target, _) = watch::channel(None);
        let inner = Arc::new(Inner {
            tokens: Mutex::new(HashMap::new()),
            target,
            stats: Mutex::new(Stats::default()),
            clients: AtomicUsize::new(0),
        });
        let i2 = inner.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                tokio::spawn(handle(s, i2.clone()));
            }
        });
        Ok(Self { port, inner })
    }

    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The emulator to show (`None`: no device; windows show "waiting").
    pub fn set_target(&self, t: Option<Target>) {
        self.inner.target.send_if_modified(|cur| {
            if *cur == t {
                false
            } else {
                *cur = t;
                true
            }
        });
    }

    #[must_use]
    pub fn target(&self) -> Option<Target> {
        self.inner.target.borrow().clone()
    }

    /// A single-use connection token; `interact` lets the window send input.
    #[must_use]
    pub fn grant(&self, interact: bool) -> String {
        let t = token();
        let mut tokens = self
            .inner
            .tokens
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        tokens.retain(|_, (_, exp)| *exp > now);
        tokens.insert(t.clone(), (interact, now + TOKEN_TTL));
        t
    }

    #[must_use]
    pub fn url(&self, token: &str) -> String {
        format!("ws://127.0.0.1:{}/preview?token={token}", self.port)
    }

    #[must_use]
    pub fn clients(&self) -> usize {
        self.inner.clients.load(Ordering::SeqCst)
    }

    #[must_use]
    pub fn stats(&self) -> Report {
        self.inner
            .stats
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .report()
    }
}

struct ClientGuard(Arc<Inner>);

impl Drop for ClientGuard {
    fn drop(&mut self) {
        self.0.clients.fetch_sub(1, Ordering::SeqCst);
    }
}

fn forbidden(why: &str) -> ErrorResponse {
    let mut r = http::Response::new(Some(why.to_string()));
    *r.status_mut() = http::StatusCode::FORBIDDEN;
    r
}

/// Streams frames of `emu` at `size` into `out` until aborted or the stream ends.
/// What part of the screen the window shows: the whole screen, or the cover screen of a closed
/// foldable (`screen` size, `view` rectangle).
#[derive(Debug, Clone, Copy)]
struct Crop {
    screen: (u32, u32),
    view: (u32, u32, u32, u32),
}

fn spawn_frames(
    emu: Emulator,
    size: (u32, u32),
    crop: Option<Crop>,
    out: mpsc::Sender<Message>,
    inner: Arc<Inner>,
) -> tokio::task::JoinHandle<()> {
    // with a cover screen, ask for the whole screen large enough that its cover part has `size`
    // (the emulator may send only the cover part; then it is just scaled)
    let request = crop.map_or(size, |c| {
        let scale = |v: u32, num: u32, den: u32| {
            u32::try_from(
                (u64::from(v) * u64::from(num) / u64::from(den.max(1))).min(u64::from(num)),
            )
            .unwrap_or(num)
        };
        (
            scale(size.0, c.screen.0, c.view.2)
                .min(c.screen.0)
                .max(size.0),
            scale(size.1, c.screen.1, c.view.3)
                .min(c.screen.1)
                .max(size.1),
        )
    });
    tokio::spawn(async move {
        let mut s = match emu.stream_screenshots(true, request.0, request.1).await {
            Ok(s) => s,
            Err(e) => {
                let _ = out
                    .send(Message::Text(
                        serde_json::json!({ "type": "note", "message": e }).to_string(),
                    ))
                    .await;
                return;
            }
        };
        let mut seq = 0u32;
        while let Ok(Some(img)) = s.message().await {
            let t0 = Instant::now();
            let (w, h) = img.format.as_ref().map_or(request, |f| (f.width, f.height));
            // the emulator sent the whole screen: cut out the cover screen here
            let cut = crop.and_then(|c| protocol::crop_view(&img.image, (w, h), c.screen, c.view));
            let bytes = match &cut {
                Some((rgb, cw, ch)) => protocol::frame(*cw, *ch, seq, now_us(), rgb),
                None => protocol::frame(w, h, seq, now_us(), &img.image),
            };
            seq = seq.wrapping_add(1);
            match out.try_send(Message::Binary(bytes)) {
                Ok(()) => inner
                    .stats
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .frame(us(t0.elapsed())),
                // the window is still busy with the previous frame: skip, never queue latency
                Err(mpsc::error::TrySendError::Full(_)) => {
                    inner
                        .stats
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .dropped += 1;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => return,
            }
        }
    })
}

#[allow(clippy::too_many_lines, clippy::result_large_err)] // one connection's life; tungstenite's handshake error type is large
async fn handle(stream: TcpStream, inner: Arc<Inner>) {
    let _ = stream.set_nodelay(true);
    let mut grant: Option<bool> = None;
    let callback = |req: &Request, resp: Response| -> Result<Response, ErrorResponse> {
        if req.uri().path() != "/preview" {
            return Err(forbidden("unknown path"));
        }
        let origin = req.headers().get("origin").and_then(|v| v.to_str().ok());
        if !origin_allowed(origin) {
            return Err(forbidden("origin not allowed"));
        }
        let q = req.uri().query().unwrap_or("");
        let tok = query(q, "token").ok_or_else(|| forbidden("token missing"))?;
        let mut tokens = inner.tokens.lock().unwrap_or_else(PoisonError::into_inner);
        match tokens.remove(tok) {
            Some((interact, exp)) if exp > Instant::now() => {
                grant = Some(interact);
                Ok(resp)
            }
            _ => Err(forbidden("token invalid or used")),
        }
    };
    let Ok(ws) = tokio_tungstenite::accept_hdr_async(stream, callback).await else {
        return;
    };
    let interact = grant.unwrap_or(false);
    inner.clients.fetch_add(1, Ordering::SeqCst);
    let _guard = ClientGuard(inner.clone());
    let (mut sink, mut source) = ws.split();
    // small queue: frames beyond it are skipped (see spawn_frames)
    let (out, mut out_rx) = mpsc::channel::<Message>(3);
    let writer = tokio::spawn(async move {
        while let Some(m) = out_rx.recv().await {
            if sink.send(m).await.is_err() {
                break;
            }
        }
        let _ = sink.close().await;
    });
    let mut target_rx = inner.target.subscribe();
    let mut canvas = (540u32, 1200u32);
    'device: loop {
        let target = target_rx.borrow_and_update().clone();
        let Some(t) = target else {
            let _ = out
                .send(Message::Text(r#"{"type":"waiting"}"#.into()))
                .await;
            tokio::select! {
                r = target_rx.changed() => if r.is_err() { break 'device; },
                m = source.next() => match m {
                    Some(Ok(Message::Text(txt))) => {
                        if let Ok(ClientMsg::Resize { w, h }) = serde_json::from_str::<ClientMsg>(&txt) {
                            canvas = (w, h);
                        }
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => break 'device,
                    _ => {}
                },
            }
            continue;
        };
        // AUD-P1-42/43: closed on its cover screen, the window sees only that screen
        let (vx, vy, vw, vh) = t.view.unwrap_or((0, 0, t.width, t.height));
        let device = (vw, vh);
        let offset = (vx, vy);
        let crop = t.view.map(|view| Crop {
            screen: (t.width, t.height),
            view,
        });
        let mut size = protocol::stream_size(device, canvas);
        let hello = |size: (u32, u32)| {
            serde_json::to_string(&Hello {
                kind: "hello",
                serial: t.serial.clone(),
                device_width: vw,
                device_height: vh,
                width: size.0,
                height: size.1,
                interact,
            })
            .unwrap_or_default()
        };
        let _ = out.send(Message::Text(hello(size))).await;
        let emu = Emulator::new(t.grpc_port, t.grpc_token.clone());
        let input = if interact {
            emu.input_stream().await.ok()
        } else {
            None
        };
        let mut frames = spawn_frames(emu.clone(), size, crop, out.clone(), inner.clone());
        loop {
            tokio::select! {
                r = target_rx.changed() => {
                    frames.abort();
                    if r.is_err() { break 'device; }
                    continue 'device;
                }
                m = source.next() => match m {
                    Some(Ok(Message::Text(txt))) => {
                        let received = Instant::now();
                        let Ok(msg) = serde_json::from_str::<ClientMsg>(&txt) else { continue };
                        match &msg {
                            ClientMsg::Resize { w, h } => {
                                canvas = (*w, *h);
                                let next = protocol::stream_size(device, canvas);
                                if next != size {
                                    size = next;
                                    frames.abort();
                                    let _ = out.send(Message::Text(hello(size))).await;
                                    frames = spawn_frames(emu.clone(), size, crop, out.clone(), inner.clone());
                                }
                            }
                            ClientMsg::Stats(ui) => {
                                inner.stats.lock().unwrap_or_else(PoisonError::into_inner).ui = *ui;
                            }
                            ClientMsg::Finger { down, id } => {
                                // viewing-only windows cannot touch the sensor either
                                if interact {
                                    if let Err(e) = emu.fingerprint_async(*down, *id).await {
                                        let _ = out.send(Message::Text(serde_json::json!({ "type": "note", "message": e }).to_string())).await;
                                    }
                                }
                            }
                            _ => {
                                if let (Some(tx), Some(ev)) = (&input, protocol::to_input(&msg, device).map(|e| protocol::offset_touch(e, offset))) {
                                    if tx.send(ev).await.is_ok() {
                                        inner.stats.lock().unwrap_or_else(PoisonError::into_inner).input(us(received.elapsed()));
                                    }
                                }
                            }
                        }
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => {
                        frames.abort();
                        break 'device;
                    }
                    _ => {}
                },
            }
        }
    }
    drop(out);
    let _ = writer.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;

    #[test]
    fn origins_and_query() {
        assert!(origin_allowed(None));
        assert!(origin_allowed(Some("http://tauri.localhost")));
        assert!(!origin_allowed(Some("https://evil.example")));
        assert!(!origin_allowed(Some("http://127.0.0.1:8080")));
        assert!(origin_allowed(Some("vscode-webview://1a2b3c-4d5e")));
        assert!(!origin_allowed(Some("vscode-webview://")));
        assert!(!origin_allowed(Some("vscode-webview://x.evil.example/")));
        assert_eq!(query("a=1&token=abc&w=2", "token"), Some("abc"));
        assert_eq!(query("tokenx=1", "token"), None);
        assert_eq!(token().len(), 32);
        assert_ne!(token(), token());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn tokens_are_single_use_and_origins_checked() {
        let srv = PreviewServer::start().await.unwrap();
        // no token
        let bad =
            tokio_tungstenite::connect_async(format!("ws://127.0.0.1:{}/preview", srv.port()))
                .await;
        assert!(bad.is_err());
        // a valid token connects once; with no device the window is told to wait
        let tok = srv.grant(true);
        let (mut ws, _) = tokio_tungstenite::connect_async(srv.url(&tok))
            .await
            .unwrap();
        let first = ws.next().await.unwrap().unwrap();
        assert_eq!(first.to_text().unwrap(), r#"{"type":"waiting"}"#);
        assert_eq!(srv.clients(), 1);
        assert!(
            tokio_tungstenite::connect_async(srv.url(&tok))
                .await
                .is_err(),
            "used token"
        );
        // a foreign web page is refused even with a fresh token
        let tok2 = srv.grant(false);
        let mut req = srv.url(&tok2).into_client_request().unwrap();
        req.headers_mut()
            .insert("origin", "https://evil.example".parse().unwrap());
        assert!(tokio_tungstenite::connect_async(req).await.is_err());
        ws.close(None).await.unwrap();
        let start = Instant::now();
        while srv.clients() != 0 && start.elapsed() < Duration::from_secs(5) {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(srv.clients(), 0);
    }
}
