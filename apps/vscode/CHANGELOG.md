# Changelog

## 0.1.4

- The Mobile Preview opens to the left of your code. Move it anywhere afterwards; it stays there.
- Connect Copilot now also adds the error loop: when Copilot's agent finishes a task and the app still has a build error, crash or runtime error, it is sent back to fix it (a long Gradle build is waited for first).
- Licensed under Apache-2.0.
- Needs Toolchain Universal Mobile Runtime 0.1.33 or later.

## 0.1.3

- New name: Toolchain Universal Mobile Runtime. The extension is now `tumr.tumr-vscode` and runs `tumr.exe` (the old `umr-vscode` extension can be uninstalled). Commands start with `tumr.`.
- Needs Toolchain Universal Mobile Runtime 0.1.32 or later.

## 0.1.2

- Foldable phones: each half turns on the hinge as you open or close it (fold like a book, flip like a laptop), with posture steps at 180°, 120°, 90°, 60° and 30° and the cover screen when closed.
- Front camera and side fingerprint key placed as on current foldables.
- Phone frame now fits the screen exactly, so the picture has no thin black bars.
- The preview needs Toolchain Universal Mobile Runtime 0.1.27 or later for the foldable controls.

## 0.1.1

- Mobile Preview uses the runtime's colours, light or dark as Windows is set.
- Clear toolbar icons (back, home, recent apps, rotate, screenshot, reload, self-test, problems) and a red Stop button that stops the app while the emulator stays on.
- In-display fingerprint sensor and phone frame as in the desktop app.

## 0.1.0

- First version: Run on Emulator, Mobile Preview panel (live screen, input, phone body, fingerprint sensor), reload / restart / rebuild / stop, runtime problems in the Problems panel, status bar, Connect Copilot.
