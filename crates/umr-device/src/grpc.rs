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

    /// `getStatus`: emulator version, uptime (ms), booted.
    pub fn status(&self) -> Res<EmulatorStatus> {
        block(self.unary::<Empty, EmulatorStatus>("getStatus", Empty {}))
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
    }

    #[test]
    fn unreachable_endpoint_is_an_error() {
        // port 1: nothing listens there
        let e = Emulator::new(1, None).status().unwrap_err();
        assert!(e.contains("port 1"), "{e}");
    }
}
