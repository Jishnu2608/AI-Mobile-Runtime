# POC-012 — Host-side emulator frame stream (gRPC) in Tauri 2

**Date:** 2026-11-11 · **Host:** Windows 11, Intel Core Ultra 7 258V (8 cores), Intel Arc 140V
**Code (throwaway):** `poc/preview/grpcbridge` (Rust, tonic 0.12 + prost 0.13 with hand-written messages, no `protoc`), `poc/preview/web/index.html` (raw RGBA path), `poc/preview/sweep-grpc.ps1`; same Tauri `viewer` as POC-003.
**Device:** AVD `medium_phone` (Android 16, 1080×2400, google_apis, 4 GB), emulator 37.2.13 with `-gpu host -grpc 8554 -grpc-use-token`.
**Status:** **Frame-rate target met** at window-sized streams (44–51 fps). Latency p95 172–230 ms: close to, but not inside, the 150 ms target.

## Architecture tested
```
emulator (host process) ── gRPC EmulatorController.streamScreenshot(RGBA8888, w×h) ── 127.0.0.1:8554 (Bearer token)
   → grpcbridge.exe: frame → WebSocket [0x03][w][h][seq][tsUs][RGBA]  (broadcast queue of 4: stale frames dropped)
   → viewer.exe (Tauri 2 / WebView2): putImageData on canvas
   ← input: same scrcpy-style messages from the page → grpcbridge → sendTouch / sendKey (unary gRPC)
```
No guest video encoding. Emulator scales the frame to the requested size on the host.

## Results (pointer_location overlay on, 40 taps + 10 s continuous drag per size)
| Stream size | Latency p50 | Latency p95 | FPS during drag | qemu host cores | grpcbridge CPU / RAM |
|---|---|---|---|---|---|
| 540×1200 | 104.2 ms | 221.7 ms | **50.9** | 1.61 | 0.18 cores / 17 MB |
| 720×1600 | 133.8 ms | **172.5 ms** | **44.4** | 1.41 | 0.27 cores / 22 MB |
| 1080×2400 | 185.8 ms | 230.3 ms | 21.8 | 1.12 | 0.24 cores / 38 MB |

Comparison with POC-003 (scrcpy, guest software encoder, same overlay): 576×1280 → 10.5 fps / p95 149 ms; 864×1920 → 8.0 fps / p95 177 ms.

## Findings
1. **gRPC host-side capture removes the encoder bottleneck.** 4–5× higher frame rate than scrcpy at comparable sizes, with lower guest CPU.
2. **Stream at window size, not device size.** The docked preview window is ~400–500 px wide, so 540×1200 or 720×1600 is enough. Full 1080×2400 RGBA is ~10 MB/frame (~220 MB/s at 22 fps) and halves the frame rate.
3. **Latency is ~170–230 ms p95.** Likely contributors (not yet isolated): one unary gRPC call per touch event, Android input dispatch + pointer_location redraw, frame cadence, and `putImageData` on the WebView main thread. Candidate fixes: WebGL texture upload or `OffscreenCanvas` in a worker, streaming input RPC (`streamInputEvent`, if available in this emulator version), smaller stream while interacting.
4. **Authentication works.** `-grpc-use-token` writes the token to `%LOCALAPPDATA%\Temp\avd\running\pid_<pid>.ini` (`grpc.token`, `grpc.port`). Bridge sends `authorization: Bearer <token>`. Product must read this file per emulator instance and never expose the token to agents. The discovery path is outside our managed root (another isolation leak to track in POC-010).
5. **No `protoc` needed.** A hand-written prost subset of `emulator_controller.proto` (shipped in `emulator/lib/`) was enough. Product can generate full bindings from the shipped proto at build time.
6. **Frame-dropping queue** (broadcast capacity 4) keeps latency bounded when the WebView is slower than the stream.
7. **Physical devices still need scrcpy** (no emulator gRPC there). The product needs two frame sources behind one stream interface: emulator → gRPC, device → scrcpy.

## Assessment against NFR-02/03
| Criterion | Result |
|---|---|
| ≥ 30 fps sustained | **Met** at 540×1200 (50.9) and 720×1600 (44.4) |
| p95 ≤ 150 ms | **Not met** (172.5 ms best); p50 104–134 ms |

**Recommendation:** adopt **emulator gRPC `streamScreenshot` as the default emulator preview source**, at window size (default 720×1600 class), with scrcpy kept for physical devices. Open a follow-up (POC-013) to cut latency: WebGL/worker rendering and streaming input, and re-measure without the pointer_location overlay using a lighter feedback method.

## Cleanup
Emulator, grpcbridge, viewer, adb stopped; pointer_location reset to 0.
