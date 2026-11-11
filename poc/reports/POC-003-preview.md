# POC-003 — Embedded preview latency in Tauri 2 (WebView2)

**Date:** 2026-11-11 · **Host:** Windows 11, Intel Core Ultra 7 258V (8 cores), Intel Arc 140V, ~31.5 GB RAM
**Code (throwaway):** `poc/preview/` — `bridge` (Rust, tokio, tokio-tungstenite), `viewer` (Tauri 2), `web/index.html` (WebCodecs), `sweep.ps1`
**Device:** AVD `medium_phone` (Android 16, 1080×2400, 420 dpi) in `D:\UMR-poc`
**Status:** Pipeline **works end-to-end**. Latency target **met at 1280 px** and close at 1920 px. Frame-rate target (≥ 30 fps) **not met** for emulator targets at useful resolutions — limited by the emulator's software H.264 encoder, not by Tauri/WebView2.

## Architecture tested
```
emulator (guest) ── scrcpy-server v5.0.1 (H.264, guest encoder) ── adb forward tcp:27183
      → bridge.exe (Rust): parses session/media packets → WebSocket 127.0.0.1:27201 (binary)
      → viewer.exe (Tauri 2, WebView2 / Edge 157): WebCodecs VideoDecoder (Annex B, optimizeForLatency) → canvas 2D
      ← control: pointer/keyboard/wheel → scrcpy control messages (built in JS) → WebSocket → bridge → control socket
```
scrcpy-server v5.0.1 downloaded from GitHub release, SHA-256 `764eb6f7…bc536` verified.

## Results (final configuration: `-gpu host`, google_apis image, 4 GB guest RAM, guest idle before test)
Latency = time from sending a touch-down to the first decoded frame where the `pointer_location` status bar changed (includes device input handling, rendering, encoding, transport, decoding, canvas draw; excludes monitor scan-out). 40 taps per run.

| max_size (stream res.) | Latency p50 | Latency p95 | FPS during continuous drag | qemu host cores |
|---|---|---|---|---|
| 720 (324×720) | 109.7 ms | 320.7 ms | 21.0 | 2.08 |
| 1280 (576×1280) | 132.8 ms | **149.2 ms** | 10.5 | 1.87 |
| 1920 (864×1920) | 140.0 ms | 177.4 ms | 8.0 | 2.07 |

Without the `pointer_location` overlay (earlier runs, same config): 720 → **54.1 fps** (first run) / 32.7 fps; 1024 → 22.2 fps; 1280 → 19.7 fps.

Footprint (1920 run): bridge 7–8 MB RAM, ~0.3–1.1 CPU-s per 90 s; viewer.exe 27–81 MB; WebView2 processes for our window 645–834 MB; qemu 3.7–5.2 GB.
Build: release build of both crates 9 min 44 s first time; viewer.exe 8.7 MB, bridge.exe 1.3 MB.

## Findings
1. **Tauri 2 + WebView2 + WebCodecs works** for H.264 Annex-B decode with `optimizeForLatency`, hardware decode preferred. Decode/draw is not the bottleneck: bridge packet rate ≈ decoded frame rate in every run; 54 fps decoded at 720 px. → **Confirms the Tauri decision (Q4)** for Windows.
2. **Bottleneck = emulator video encoding.** The emulator exposes only software encoders (`c2.android.avc.encoder (sw)`, also hevc/av1/vp8/vp9 sw). Frame rate falls with resolution and guest load. Real devices have hardware encoders, so scrcpy should perform much better on physical phones (future scope).
3. **Headless emulator defaults to software GPU (SwiftShader).** Must launch with `-gpu host` (Intel Arc used). With SwiftShader: 2.9 fps, ~700 ms latency.
4. **Default `medium_phone` AVD (Play Store image, 2 GB RAM) swaps heavily** (934 MB swap, 131 % sys CPU). Use `google_apis` image and ≥ 4 GB guest RAM for our profiles. First boot after wipe keeps the guest busy for several minutes (Play services setup) — take a snapshot after warm-up.
5. **scrcpy v5 protocol details (verified):** forward tunnel → client must open video **and** control sockets before the server sends the dummy byte; 64-byte device name; `u32` codec id; 12-byte session packet (MSB=1, width, height) at start and on rotation; 12-byte media header (config bit 62, key bit 61, 61-bit PTS). Control message types: keycode 0, text 1, touch 2, scroll 3, back 4, rotate 11, reset video 17. Touch with pointer id −1 is a **mouse** (no touch feedback); use a finger pointer id (0) for touch.
6. **Latency meter lessons:** `show_touches` dot was not reliably detectable; `pointer_location` status bar works but adds rendering load (lower fps). Product does not need this; it is test-only.

## Assessment against acceptance criteria (master_build NFR-02/03)
| Criterion | Result |
|---|---|
| p95 ≤ 150 ms | **Met at 1280 px (149 ms)**; 177 ms at 1920 px; 321 ms at 720 px (outliers under load) |
| ≥ 30 fps sustained | **Not met** at ≥ 1024 px (8–22 fps); met only at 720 px without overlay |
| Input mapping (touch, drag, wheel, keys, back/home, rotate) | Implemented; touch/drag/keys verified by measurement; rotate/wheel not measured |

**Verdict:** Tauri is **not** the problem — keep Tauri. The scrcpy-over-guest-encoder path is **insufficient for emulators** at phone resolutions. Next experiment must test a host-side frame source.

## Recommended next step — POC-012 (new)
Test the **emulator gRPC frame stream** (host-side capture with `-grpc <port>`, `streamScreenshot` RPC with RGB/raw frames, or the emulator WebRTC module) feeding the same Tauri viewer:
- no guest encoding; host GPU readback;
- measure fps and latency at 1080×2400 and at half resolution;
- transport options: raw RGB over local WebSocket into `ImageData`/WebGL, or host-side H.264 encoding with the GPU (Media Foundation) if bandwidth is too high.
Keep scrcpy for **physical devices** (future) where hardware encoders exist.

## Cleanup
Emulator, bridge, viewer, adb stopped. `pointer_location` and `show_touches` reset to 0. AVD config changed to google_apis + 4 GB (`config.ini.orig` kept). `poc/preview/target` = 1.47 GB (git-ignored).
