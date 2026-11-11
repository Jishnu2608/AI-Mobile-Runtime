# POC-013 — Preview latency breakdown and reduction

**Date:** 2026-11-11 · **Host:** Windows 11, Intel Core Ultra 7 258V, Intel Arc 140V
**Code (throwaway):** `poc/preview/grpcbridge` (adds `INPUT_MODE=stream` → `streamInputEvent`, and sendTouch RPC timing), `poc/preview/web/index.html` (adds `render=gl` WebGL2 path, per-frame draw timing, detection on received frame bytes), `poc/preview/viewer` (`VIEW_QUERY` env), `poc/preview/sweep-013.ps1`
**Device:** AVD `medium_phone`, emulator 37.2.13, `-gpu host -grpc 8554 -grpc-use-token`, guest idle before runs, `pointer_location` overlay used as touch feedback.
**Status:** Latency sources isolated. **Our pipeline adds only ~2–4 ms.** The remaining ~100–130 ms p50 is inside the emulator/Android. Best variant reaches p95 ≈ 145–148 ms in 3 of 4 runs, but run-to-run noise (p95 145–197 ms) means NFR-02 cannot be guaranteed by our code alone.

## Measured components
| Component | Measurement |
|---|---|
| Input: unary `sendTouch` gRPC round trip | p50 1.2–1.8 ms, p95 3–14 ms (1,200 events) |
| Input: `streamInputEvent` (client stream) | no per-call wait; not separately measurable |
| Render: 2D `putImageData` 720×1600 | p50 0.5–0.6 ms, p95 0.9–1.0 ms |
| Render: WebGL2 `texSubImage2D` + draw 720×1600 | p50 0.4 ms, p95 0.7 ms |
| Render: WebGL2 540×1200 | p50 0.3 ms, p95 0.5 ms |
| End-to-end (touch sent → frame with feedback drawn) | p50 102–133 ms (all variants) |

## A/B results (40 taps per run)
| Stream | Render | Input | Latency p50 | Latency p95 | FPS (drag) |
|---|---|---|---|---|---|
| 720×1600 | 2d | unary | 112.2 | 174.3 | 47.5 |
| 720×1600 | gl | unary | 118.8 | 158.4 | 47.6 |
| 720×1600 | 2d | stream | 118.0 | 150.4 | 47.4 |
| 720×1600 | gl | stream | 111.3 | **144.9** | 47.2 |
| 540×1200 | gl | stream | 102.3 | **144.8** | 51.8 |
| Repeat 1: 720×1600 | gl | stream | 130.1 | 196.7 | 33.7 |
| Repeat 1: 720×1600 | 2d | unary | 127.8 | 158.6 | 33.6 |
| Repeat 2: 720×1600 | gl | stream | 126.4 | **148.3** | 31.9 |
| Repeat 2: 720×1600 | 2d | unary | 133.0 | 164.6 | 31.6 |

During the repeat runs the whole host was slower (FPS fell from ~47 to ~32 for every variant), which shows how much host load and thermal state affect the emulator.

## Findings
1. **The bottleneck is not ours.** Input RPC (~1.5 ms) + draw (< 1 ms) + WebSocket hop (loopback) add only a few ms. The ~100+ ms floor comes from Android input dispatch, app/overlay rendering at 60 Hz (1–2 frames), and the emulator's screenshot stream cadence. The `pointer_location` overlay itself is a heavy test-only redraw.
2. **WebGL + streaming input is the best variant**: lowest p95 in 3 of 4 runs and lower main-thread cost. Gains are small (≈ 10–25 ms on p95) and within noise on a loaded host.
3. **Smaller stream helps a little** (540×1200: p50 102 ms, 51.8 fps).
4. **Host load dominates variance.** Same variant: p95 145 → 197 ms between runs.

## Recommendations
1. Adopt for the product: **WebGL2 rendering** of RGBA frames and **`streamInputEvent`** for input; stream at window size.
2. **Revise NFR-02** (owner decision): measure what the product controls. Proposal:
   - NFR-02a: pipeline-added latency (input received by Core → event delivered to emulator, plus frame received → drawn) **p95 ≤ 20 ms** — met (≈ 2–15 ms).
   - NFR-02b: end-to-end touch-to-visible on reference hardware with an idle emulator **p95 ≤ 200 ms** — met in all runs (145–197 ms).
   - Keep 150 ms p95 as a stretch goal, re-measured on physical devices (scrcpy, hardware encoder) later.
3. Add a built-in latency self-test to the product's diagnostics (same method) so users can see when the host is overloaded.

## Cleanup
Emulator, bridges, viewer, adb stopped; `pointer_location` reset to 0.
