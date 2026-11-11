//! POC-012 bridge (throwaway): Android Emulator gRPC `streamScreenshot` -> WebSocket.
//!
//! Frames are captured host-side by the emulator (no guest video encoder).
//! Env: GRPC (default http://127.0.0.1:8554), GRPC_TOKEN (optional bearer token),
//!      WEB_DIR, OUT_W/OUT_H (stream size, default 540x1200), DEV_W/DEV_H (device size, default 1080x2400)
//! WS out: [0x01][w u32][h u32] session, [0x03][w u32][h u32][seq u32][tsUs u64][RGBA bytes]
//! WS in : scrcpy-style control messages from the same viewer page (touch=2, keycode=0, text=1),
//!         translated to emulator sendTouch / sendKey.

use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::tungstenite::Message;
use tonic::codec::ProstCodec;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

type Res<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

// ---- hand-written subset of emulator_controller.proto (package android.emulation.control) ----
#[derive(Clone, PartialEq, prost::Message)]
pub struct ImageFormat {
    #[prost(int32, tag = "1")] pub format: i32, // 1 = RGBA8888
    #[prost(uint32, tag = "3")] pub width: u32,
    #[prost(uint32, tag = "4")] pub height: u32,
    #[prost(uint32, tag = "5")] pub display: u32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Image {
    #[prost(message, optional, tag = "1")] pub format: Option<ImageFormat>,
    #[prost(bytes = "vec", tag = "4")] pub image: Vec<u8>,
    #[prost(uint32, tag = "5")] pub seq: u32,
    #[prost(uint64, tag = "6")] pub timestamp_us: u64,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Touch {
    #[prost(int32, tag = "1")] pub x: i32,
    #[prost(int32, tag = "2")] pub y: i32,
    #[prost(int32, tag = "3")] pub identifier: i32,
    #[prost(int32, tag = "4")] pub pressure: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct TouchEvent {
    #[prost(message, repeated, tag = "1")] pub touches: Vec<Touch>,
    #[prost(int32, tag = "2")] pub display: i32,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct KeyboardEvent {
    #[prost(int32, tag = "1")] pub code_type: i32,
    #[prost(int32, tag = "2")] pub event_type: i32, // 2 = keypress
    #[prost(int32, tag = "3")] pub key_code: i32,
    #[prost(string, tag = "4")] pub key: String,
    #[prost(string, tag = "5")] pub text: String,
}
#[derive(Clone, PartialEq, prost::Message)]
pub struct Empty {}
/// `InputEvent` oneof modelled as optional fields (same wire format).
#[derive(Clone, PartialEq, prost::Message)]
pub struct InputEvent {
    #[prost(message, optional, tag = "1")] pub key_event: Option<KeyboardEvent>,
    #[prost(message, optional, tag = "2")] pub touch_event: Option<TouchEvent>,
}

const SVC: &str = "/android.emulation.control.EmulatorController/";

#[derive(Clone)]
struct Emu { ch: Channel, token: Option<String> }

impl Emu {
    fn req<T>(&self, m: T) -> tonic::Request<T> {
        let mut r = tonic::Request::new(m);
        if let Some(t) = &self.token {
            r.metadata_mut().insert("authorization", MetadataValue::try_from(format!("Bearer {t}")).unwrap());
        }
        r
    }
    async fn unary<Q: prost::Message + Default + 'static, A: prost::Message + Default + 'static>(&self, method: &str, m: Q) -> Res<A> {
        let mut g = tonic::client::Grpc::new(self.ch.clone());
        g.ready().await?;
        let path: http::uri::PathAndQuery = format!("{SVC}{method}").parse()?;
        Ok(g.unary(self.req(m), path, ProstCodec::<Q, A>::default()).await?.into_inner())
    }
    /// Opens client-streaming `streamInputEvent`; returns a sender for events.
    async fn input_stream(&self) -> Res<mpsc::Sender<InputEvent>> {
        let (tx, rx) = mpsc::channel::<InputEvent>(256);
        let mut g = tonic::client::Grpc::new(self.ch.clone());
        g.ready().await?;
        let path: http::uri::PathAndQuery = format!("{SVC}streamInputEvent").parse()?;
        let req = self.req(tokio_stream::wrappers::ReceiverStream::new(rx));
        tokio::spawn(async move {
            let r = g.client_streaming(req, path, ProstCodec::<InputEvent, Empty>::default()).await;
            println!("[grpc] input stream ended: {:?}", r.err());
        });
        Ok(tx)
    }
    async fn stream_screens(&self, f: ImageFormat) -> Res<tonic::Streaming<Image>> {
        let mut g = tonic::client::Grpc::new(self.ch.clone()).max_decoding_message_size(64 << 20);
        g.ready().await?;
        let path: http::uri::PathAndQuery = format!("{SVC}streamScreenshot").parse()?;
        Ok(g.server_streaming(self.req(f), path, ProstCodec::<ImageFormat, Image>::default()).await?.into_inner())
    }
}

fn env_u32(k: &str, d: u32) -> u32 { std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d) }

#[tokio::main]
async fn main() -> Res<()> {
    let addr = std::env::var("GRPC").unwrap_or_else(|_| "http://127.0.0.1:8554".into());
    let token = std::env::var("GRPC_TOKEN").ok().filter(|t| !t.is_empty());
    let web_dir = std::env::var("WEB_DIR")?;
    let (out_w, out_h) = (env_u32("OUT_W", 540), env_u32("OUT_H", 1200));
    let (dev_w, dev_h) = (env_u32("DEV_W", 1080), env_u32("DEV_H", 2400));

    let ch = Channel::from_shared(addr.clone())?.connect().await?;
    let emu = Emu { ch, token };
    println!("[grpc] connected {addr} token={}", emu.token.is_some());

    // One-shot mode: SHOT=<file.png> saves a host-side PNG screenshot and exits.
    if let Ok(path) = std::env::var("SHOT") {
        let t = std::time::Instant::now();
        let img: Image = emu.unary("getScreenshot", ImageFormat { format: 0, width: 0, height: 0, display: 0 }).await?;
        tokio::fs::write(&path, &img.image).await?;
        println!("[grpc] screenshot {} bytes in {} ms -> {path}", img.image.len(), t.elapsed().as_millis());
        return Ok(());
    }

    let (tx, _) = broadcast::channel::<Arc<Vec<u8>>>(4); // small: drop old frames instead of queueing
    let mut session = vec![1u8];
    session.extend_from_slice(&out_w.to_be_bytes());
    session.extend_from_slice(&out_h.to_be_bytes());
    let session = Arc::new(session);

    // frame pump
    {
        let tx = tx.clone();
        let emu = emu.clone();
        tokio::spawn(async move {
            let fmt = ImageFormat { format: 1, width: out_w, height: out_h, display: 0 };
            let mut s = match emu.stream_screens(fmt).await { Ok(s) => s, Err(e) => { println!("[grpc] stream error {e}"); return; } };
            let (mut n, t0) = (0u64, std::time::Instant::now());
            while let Ok(Some(img)) = s.message().await {
                let (w, h) = img.format.as_ref().map(|f| (f.width, f.height)).unwrap_or((out_w, out_h));
                let mut m = Vec::with_capacity(21 + img.image.len());
                m.push(3u8);
                m.extend_from_slice(&w.to_be_bytes());
                m.extend_from_slice(&h.to_be_bytes());
                m.extend_from_slice(&img.seq.to_be_bytes());
                m.extend_from_slice(&img.timestamp_us.to_be_bytes());
                m.extend_from_slice(&img.image);
                let _ = tx.send(Arc::new(m));
                n += 1;
                if n % 300 == 0 { println!("[grpc] frames={n} avg_fps={:.1} size={w}x{h}", n as f64 / t0.elapsed().as_secs_f64()); }
            }
            println!("[grpc] stream ended");
        });
    }

    // input translator
    let (ctl_tx, mut ctl_rx) = mpsc::channel::<Vec<u8>>(256);
    let stream_mode = std::env::var("INPUT_MODE").map(|v| v == "stream").unwrap_or(false);
    let in_tx = if stream_mode { Some(emu.input_stream().await?) } else { None };
    println!("[grpc] input mode = {}", if stream_mode { "streamInputEvent" } else { "unary sendTouch" });
    {
        let emu = emu.clone();
        tokio::spawn(async move {
            let mut rpc_ms: Vec<f64> = Vec::new();
            while let Some(b) = ctl_rx.recv().await {
                if let (Some(tx), Some(2)) = (&in_tx, b.first()) {
                    if b.len() >= 32 {
                        let x = i32::from_be_bytes(b[10..14].try_into().unwrap());
                        let y = i32::from_be_bytes(b[14..18].try_into().unwrap());
                        let w = u16::from_be_bytes(b[18..20].try_into().unwrap()) as i64;
                        let h = u16::from_be_bytes(b[20..22].try_into().unwrap()) as i64;
                        let t = Touch { x: (x as i64 * dev_w as i64 / w.max(1)) as i32, y: (y as i64 * dev_h as i64 / h.max(1)) as i32,
                                        identifier: 0, pressure: if b[1] == 1 { 0 } else { 1 } };
                        let _ = tx.send(InputEvent { touch_event: Some(TouchEvent { touches: vec![t], display: 0 }), key_event: None }).await;
                        continue;
                    }
                }
                let t0 = std::time::Instant::now();
                let is_touch = b.first() == Some(&2);
                let r: Res<()> = async {
                    match b.first() {
                        Some(2) if b.len() >= 32 => {
                            let action = b[1];
                            let x = i32::from_be_bytes(b[10..14].try_into()?);
                            let y = i32::from_be_bytes(b[14..18].try_into()?);
                            let w = u16::from_be_bytes(b[18..20].try_into()?) as i64;
                            let h = u16::from_be_bytes(b[20..22].try_into()?) as i64;
                            let dx = (x as i64 * dev_w as i64 / w.max(1)) as i32;
                            let dy = (y as i64 * dev_h as i64 / h.max(1)) as i32;
                            let pressure = if action == 1 { 0 } else { 1 };
                            emu.unary::<_, Empty>("sendTouch", TouchEvent { touches: vec![Touch { x: dx, y: dy, identifier: 0, pressure }], display: 0 }).await?;
                        }
                        Some(0) if b.len() >= 14 && b[1] == 0 => {
                            let code = i32::from_be_bytes(b[2..6].try_into()?);
                            let key = match code { 4 => "GoBack", 3 => "GoHome", 66 => "Enter", 67 => "Backspace", _ => "" };
                            if !key.is_empty() {
                                emu.unary::<_, Empty>("sendKey", KeyboardEvent { event_type: 2, key: key.into(), ..Default::default() }).await?;
                            }
                        }
                        Some(1) if b.len() > 5 => {
                            let text = String::from_utf8_lossy(&b[5..]).to_string();
                            emu.unary::<_, Empty>("sendKey", KeyboardEvent { event_type: 2, text, ..Default::default() }).await?;
                        }
                        _ => {}
                    }
                    Ok(())
                }.await;
                if let Err(e) = r { println!("[grpc] input error {e}"); }
                if is_touch {
                    rpc_ms.push(t0.elapsed().as_secs_f64() * 1000.0);
                    if rpc_ms.len() % 100 == 0 {
                        let mut s = rpc_ms.clone(); s.sort_by(|a, b| a.partial_cmp(b).unwrap());
                        println!("[grpc] sendTouch rpc n={} p50={:.2}ms p95={:.2}ms", s.len(), s[s.len() / 2], s[s.len() * 95 / 100]);
                    }
                }
            }
        });
    }

    // HTTP static (same page as POC-003)
    let web = Arc::new(web_dir);
    tokio::spawn(async move {
        let l = TcpListener::bind(("127.0.0.1", 27200)).await.unwrap();
        loop {
            let (mut s, _) = match l.accept().await { Ok(v) => v, Err(_) => continue };
            let web = web.clone();
            tokio::spawn(async move {
                let mut buf = [0u8; 2048];
                let _ = s.read(&mut buf).await;
                let body = tokio::fs::read(format!("{}/index.html", web)).await.unwrap_or_default();
                let head = format!("HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes()).await;
                let _ = s.write_all(&body).await;
            });
        }
    });

    let l = TcpListener::bind(("127.0.0.1", 27201)).await?;
    println!("[bridge] ready http://127.0.0.1:27200/  ws://127.0.0.1:27201/");
    loop {
        let (s, _) = l.accept().await?;
        let mut rx = tx.subscribe();
        let ctl_tx = ctl_tx.clone();
        let session = session.clone();
        tokio::spawn(async move {
            let ws = match tokio_tungstenite::accept_async(s).await { Ok(w) => w, Err(_) => return };
            let (mut wtx, mut wrx) = ws.split();
            let _ = wtx.send(Message::Binary((*session).clone())).await;
            let fwd = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(m) => { if wtx.send(Message::Binary((*m).clone())).await.is_err() { break; } }
                        Err(broadcast::error::RecvError::Lagged(_)) => {} // dropped stale frames
                        Err(_) => break,
                    }
                }
            });
            while let Some(Ok(msg)) = wrx.next().await {
                match msg {
                    Message::Binary(b) => { let _ = ctl_tx.send(b).await; }
                    Message::Text(t) => {
                        println!("[result] {t}");
                        if let Ok(mut f) = tokio::fs::OpenOptions::new().create(true).append(true).open("results.log").await {
                            let _ = f.write_all(format!("{t}\n").as_bytes()).await;
                        }
                    }
                    _ => {}
                }
            }
            fwd.abort();
        });
    }
}
