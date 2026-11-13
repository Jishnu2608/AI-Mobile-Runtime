//! Emulator gRPC client (DEC-007): screenshot, touch, text, posture.
//!
//! Hand-written subset of `emulator_controller.proto` (package `android.emulation.control`,
//! shipped in `<sdk>/emulator/lib/`), same wire format, no code generation. Each emulator is
//! started with `-grpc <port> -grpc-use-token`; the token comes from its discovery file and is
//! sent as `authorization: Bearer <token>`.
//!
//! The functions are blocking: the Core calls them from its blocking worker threads.

use std::future::Future;
use std::time::Duration;

use tonic::codec::ProstCodec;
use tonic::metadata::MetadataValue;
use tonic::transport::{Channel, Endpoint};

use crate::catalog::Posture;

const SVC: &str = "/android.emulation.control.EmulatorController/";

#[derive(Clone, PartialEq, prost::Message)]
pub struct ImageFormat {
    /// 0 = PNG, 1 = RGBA8888, 2 = RGB888
    #[prost(int32, tag = "1")]
    pub format: i32,
    #[prost(uint32, tag = "3")]
    pub width: u32,
    #[prost(uint32, tag = "4")]
    pub height: u32,
    #[prost(uint32, tag = "5")]
    pub display: u32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Image {
    #[prost(message, optional, tag = "1")]
    pub format: Option<ImageFormat>,
    #[prost(bytes = "vec", tag = "4")]
    pub image: Vec<u8>,
    #[prost(uint32, tag = "5")]
    pub seq: u32,
    #[prost(uint64, tag = "6")]
    pub timestamp_us: u64,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Touch {
    #[prost(int32, tag = "1")]
    pub x: i32,
    #[prost(int32, tag = "2")]
    pub y: i32,
    #[prost(int32, tag = "3")]
    pub identifier: i32,
    /// 0 = released
    #[prost(int32, tag = "4")]
    pub pressure: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct TouchEvent {
    #[prost(message, repeated, tag = "1")]
    pub touches: Vec<Touch>,
    #[prost(int32, tag = "2")]
    pub display: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct KeyboardEvent {
    #[prost(int32, tag = "1")]
    pub code_type: i32,
    /// 0 keydown, 1 keyup, 2 keypress
    #[prost(int32, tag = "2")]
    pub event_type: i32,
    #[prost(int32, tag = "3")]
    pub key_code: i32,
    #[prost(string, tag = "4")]
    pub key: String,
    #[prost(string, tag = "5")]
    pub text: String,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct PostureMsg {
    #[prost(int32, tag = "3")]
    pub value: i32,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct EmulatorStatus {
    #[prost(string, tag = "1")]
    pub version: String,
    #[prost(uint64, tag = "2")]
    pub uptime: u64,
    #[prost(bool, tag = "3")]
    pub booted: bool,
}

#[derive(Clone, PartialEq, prost::Message)]
pub struct Empty {}

#[derive(Clone, PartialEq, prost::Message)]
pub struct WheelEvent {
    #[prost(int32, tag = "1")]
    pub dx: i32,
    #[prost(int32, tag = "2")]
    pub dy: i32,
    #[prost(int32, tag = "3")]
    pub display: i32,
}

/// `InputEvent`: the proto's `oneof` as optional fields (same wire format; set exactly one).
#[derive(Clone, PartialEq, prost::Message)]
pub struct InputEvent {
    #[prost(message, optional, tag = "1")]
    pub key_event: Option<KeyboardEvent>,
    #[prost(message, optional, tag = "2")]
    pub touch_event: Option<TouchEvent>,
    #[prost(message, optional, tag = "6")]
    pub wheel_event: Option<WheelEvent>,
}

impl InputEvent {
    #[must_use]
    pub fn touch(x: i32, y: i32, pressure: i32, identifier: i32) -> Self {
        Self {
            touch_event: Some(TouchEvent {
                touches: vec![Touch {
                    x,
                    y,
                    identifier,
                    pressure,
                }],
                display: 0,
            }),
            ..Self::default()
        }
    }

    /// A W3C key (`GoBack`, `GoHome`, `AppSwitch`, `Enter`, `a`, ...); `event_type` 0 down,
    /// 1 up, 2 press.
    #[must_use]
    pub fn key(key: &str, event_type: i32) -> Self {
        Self {
            key_event: Some(KeyboardEvent {
                event_type,
                key: key.to_string(),
                ..KeyboardEvent::default()
            }),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn text(text: &str) -> Self {
        Self {
            key_event: Some(KeyboardEvent {
                event_type: 2,
                text: text.to_string(),
                ..KeyboardEvent::default()
            }),
            ..Self::default()
        }
    }

    #[must_use]
    pub fn wheel(dx: i32, dy: i32) -> Self {
        Self {
            wheel_event: Some(WheelEvent { dx, dy, display: 0 }),
            ..Self::default()
        }
    }
}

/// Runs a future to completion from blocking code (inside or outside a Tokio runtime).
fn block<F: Future>(f: F) -> F::Output {
    match tokio::runtime::Handle::try_current() {
        Ok(h) => tokio::task::block_in_place(|| h.block_on(f)),
        Err(_) => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_or_else(|e| panic!("tokio runtime: {e}"), |rt| rt.block_on(f)),
    }
}

/// Connection to one emulator's gRPC endpoint.
#[derive(Debug, Clone)]
pub struct Emulator {
    port: u16,
    token: Option<String>,
}

type Res<T> = Result<T, String>;

impl Emulator {
    #[must_use]
    pub fn new(port: u16, token: Option<String>) -> Self {
        Self { port, token }
    }

    async fn channel(&self) -> Res<Channel> {
        Endpoint::from_shared(format!("http://127.0.0.1:{}", self.port))
            .map_err(|e| e.to_string())?
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(30))
            .connect()
            .await
            .map_err(|e| format!("emulator gRPC on port {}: {e}", self.port))
    }

    async fn unary<Q, A>(&self, method: &str, msg: Q) -> Res<A>
    where
        Q: prost::Message + Default + Send + Sync + 'static,
        A: prost::Message + Default + Send + Sync + 'static,
    {
        let ch = self.channel().await?;
        let mut g = tonic::client::Grpc::new(ch).max_decoding_message_size(64 << 20);
        g.ready().await.map_err(|e| e.to_string())?;
        let path: http::uri::PathAndQuery = format!("{SVC}{method}")
            .parse()
            .map_err(|e: http::uri::InvalidUri| e.to_string())?;
        let mut req = tonic::Request::new(msg);
        if let Some(t) = &self.token {
            let v = MetadataValue::try_from(format!("Bearer {t}")).map_err(|e| e.to_string())?;
            req.metadata_mut().insert("authorization", v);
        }
        g.unary(req, path, ProstCodec::<Q, A>::default())
            .await
            .map(tonic::Response::into_inner)
            .map_err(|s| format!("emulator gRPC {method}: {}", s.message()))
    }

    /// A channel for long-lived streams (no per-request timeout).
    async fn stream_channel(&self) -> Res<Channel> {
        Endpoint::from_shared(format!("http://127.0.0.1:{}", self.port))
            .map_err(|e| e.to_string())?
            .connect_timeout(Duration::from_secs(3))
            .tcp_nodelay(true)
            .connect()
            .await
            .map_err(|e| format!("emulator gRPC on port {}: {e}", self.port))
    }

    fn authorize<T>(&self, mut req: tonic::Request<T>) -> Res<tonic::Request<T>> {
        if let Some(t) = &self.token {
            let v = MetadataValue::try_from(format!("Bearer {t}")).map_err(|e| e.to_string())?;
            req.metadata_mut().insert("authorization", v);
        }
        Ok(req)
    }

    /// PRV-008: `streamScreenshot` at `width` x `height` (0 = device size; the emulator scales on
    /// its side, so smaller windows cost less). `rgb`: RGB888 (3 bytes per pixel, 25 % less to
    /// move) instead of RGBA8888. Async: the Core's preview server drives it.
    pub async fn stream_screenshots(
        &self,
        rgb: bool,
        width: u32,
        height: u32,
    ) -> Res<tonic::Streaming<Image>> {
        let ch = self.stream_channel().await?;
        let mut g = tonic::client::Grpc::new(ch).max_decoding_message_size(64 << 20);
        g.ready().await.map_err(|e| e.to_string())?;
        let path: http::uri::PathAndQuery = format!("{SVC}streamScreenshot")
            .parse()
            .map_err(|e: http::uri::InvalidUri| e.to_string())?;
        let req = self.authorize(tonic::Request::new(ImageFormat {
            format: if rgb { 2 } else { 1 },
            width,
            height,
            display: 0,
        }))?;
        g.server_streaming(req, path, ProstCodec::<ImageFormat, Image>::default())
            .await
            .map(tonic::Response::into_inner)
            .map_err(|s| format!("emulator gRPC streamScreenshot: {}", s.message()))
    }

    /// PRV-010: `streamInputEvent` (client stream). Events sent into the returned channel reach
    /// the emulator without a round trip per event.
    pub async fn input_stream(&self) -> Res<tokio::sync::mpsc::Sender<InputEvent>> {
        let ch = self.stream_channel().await?;
        let mut g = tonic::client::Grpc::new(ch);
        g.ready().await.map_err(|e| e.to_string())?;
        let path: http::uri::PathAndQuery = format!("{SVC}streamInputEvent")
            .parse()
            .map_err(|e: http::uri::InvalidUri| e.to_string())?;
        let (tx, rx) = tokio::sync::mpsc::channel::<InputEvent>(512);
        let req = self.authorize(tonic::Request::new(
            tokio_stream::wrappers::ReceiverStream::new(rx),
        ))?;
        tokio::spawn(async move {
            let _ = g
                .client_streaming(req, path, ProstCodec::<InputEvent, Empty>::default())
                .await;
        });
        Ok(tx)
    }

    /// `getStatus`: emulator version, uptime (ms), booted.
    pub fn status(&self) -> Res<EmulatorStatus> {
        block(self.unary::<Empty, EmulatorStatus>("getStatus", Empty {}))
    }

    /// `getScreenshot` as PNG scaled by the emulator to `width` x `height` (0 = device size).
    pub fn screenshot_png_at(&self, width: u32, height: u32) -> Res<(Vec<u8>, u32, u32)> {
        let img: Image = block(self.unary(
            "getScreenshot",
            ImageFormat {
                format: 0,
                width,
                height,
                display: 0,
            },
        ))?;
        let (w, h) = img.format.map_or((0, 0), |f| (f.width, f.height));
        if img.image.is_empty() {
            return Err("the emulator returned an empty screenshot".into());
        }
        Ok((img.image, w, h))
    }

    /// `getScreenshot` as PNG; returns (png, width, height).
    pub fn screenshot_png(&self) -> Res<(Vec<u8>, u32, u32)> {
        let img: Image = block(self.unary(
            "getScreenshot",
            ImageFormat {
                format: 0,
                ..ImageFormat::default()
            },
        ))?;
        let (w, h) = img.format.map_or((0, 0), |f| (f.width, f.height));
        if img.image.is_empty() {
            return Err("the emulator returned an empty screenshot".into());
        }
        Ok((img.image, w, h))
    }

    /// `getScreenshot` as raw RGBA8888 (fast, no PNG encoding); returns (pixels, width, height).
    pub fn screenshot_rgba(&self) -> Res<(Vec<u8>, u32, u32)> {
        let img: Image = block(self.unary(
            "getScreenshot",
            ImageFormat {
                format: 1,
                ..ImageFormat::default()
            },
        ))?;
        let (w, h) = img.format.map_or((0, 0), |f| (f.width, f.height));
        Ok((img.image, w, h))
    }

    async fn touch(&self, x: i32, y: i32, pressure: i32) -> Res<()> {
        let ev = TouchEvent {
            touches: vec![Touch {
                x,
                y,
                identifier: 0,
                pressure,
            }],
            display: 0,
        };
        self.unary::<TouchEvent, Empty>("sendTouch", ev)
            .await
            .map(|_| ())
    }

    /// Tap at device pixel (x, y).
    pub fn tap(&self, x: i32, y: i32) -> Res<()> {
        block(async {
            self.touch(x, y, 1).await?;
            tokio::time::sleep(Duration::from_millis(60)).await;
            self.touch(x, y, 0).await
        })
    }

    /// Swipe from (x1, y1) to (x2, y2) over `ms` milliseconds.
    pub fn swipe(&self, from: (i32, i32), to: (i32, i32), ms: u64) -> Res<()> {
        let steps = (ms / 16).clamp(2, 120);
        block(async {
            for i in 0..=steps {
                #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
                let t = i as f64 / steps as f64;
                #[allow(clippy::cast_possible_truncation)]
                let (x, y) = (
                    (f64::from(from.0) + f64::from(to.0 - from.0) * t).round() as i32,
                    (f64::from(from.1) + f64::from(to.1 - from.1) * t).round() as i32,
                );
                self.touch(x, y, 1).await?;
                tokio::time::sleep(Duration::from_millis(ms / steps)).await;
            }
            self.touch(to.0, to.1, 0).await
        })
    }

    /// Types text into the focused field.
    pub fn text(&self, text: &str) -> Res<()> {
        block(self.unary::<KeyboardEvent, Empty>(
            "sendKey",
            KeyboardEvent {
                event_type: 2,
                text: text.to_string(),
                ..KeyboardEvent::default()
            },
        ))
        .map(|_| ())
    }

    /// `setPosture` (foldables): closed, half-opened, opened, flipped, tent.
    pub fn set_posture(&self, p: Posture) -> Res<()> {
        block(self.unary::<PostureMsg, Empty>(
            "setPosture",
            PostureMsg {
                value: i32::from(p.index()),
            },
        ))
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use prost::Message;

    #[test]
    fn wire_format_matches_the_proto() {
        // Posture.value is field 3 (varint): tag 0x18
        assert_eq!(PostureMsg { value: 2 }.encode_to_vec(), [0x18, 2]);
        // PNG request: format 0 is the default, so nothing is sent
        assert_eq!(ImageFormat::default().encode_to_vec(), Vec::<u8>::new());
        let t = TouchEvent {
            touches: vec![Touch {
                x: 5,
                y: 7,
                identifier: 0,
                pressure: 1,
            }],
            display: 0,
        };
        // field 1 (len-delimited) holding x=5 (0x08 05), y=7 (0x10 07), pressure=1 (0x20 01)
        assert_eq!(t.encode_to_vec(), [0x0a, 6, 0x08, 5, 0x10, 7, 0x20, 1]);
        let k = KeyboardEvent {
            event_type: 2,
            text: "hi".into(),
            ..KeyboardEvent::default()
        };
        assert_eq!(k.encode_to_vec(), [0x10, 2, 0x2a, 2, b'h', b'i']);
        // InputEvent.touch_event is field 2, wheel_event field 6
        let t = InputEvent::touch(5, 7, 1, 0).encode_to_vec();
        assert_eq!(t[0], 0x12);
        assert_eq!(InputEvent::wheel(0, -120).encode_to_vec()[0], 0x32);
        assert_eq!(InputEvent::key("GoBack", 2).encode_to_vec()[0], 0x0a);
    }

    #[test]
    fn unreachable_endpoint_is_an_error() {
        // port 1: nothing listens there
        let e = Emulator::new(1, None).status().unwrap_err();
        assert!(e.contains("port 1"), "{e}");
    }
}
