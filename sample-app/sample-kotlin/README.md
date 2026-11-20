# Calculator — Kotlin (Jetpack Compose)

A small calculator for Android written in **Kotlin with Jetpack Compose**, used to try the Toolchain Universal Mobile Runtime: run it on the emulator, edit it, break it on purpose and watch the runtime (and your coding agent) report the problem.

What it does: digits, `.`, `+ − × ÷`, `=`, `AC`, `⌫`, `±` and `%`, with the pending calculation shown above the result. Dividing by zero shows `Error`.

## Run it

In VS Code with the **Toolchain Universal Mobile Runtime** extension (the workspace recommends it):

1. Open this folder (`sample-app/sample-kotlin`).
2. Run **Toolchain Universal Mobile Runtime: Run on Emulator** from the Command Palette, or click **Mobile** in the status bar (bottom left) for all actions. The first time it asks to trust the folder, because building runs the project's own scripts.
3. The phone appears in the **Mobile Preview** panel. Save a file and the app updates: Kotlin has no hot reload outside Android Studio, so the runtime runs an incremental Gradle build and reinstalls (about 15–20 s).
4. Build errors and crashes appear in VS Code's **Problems** panel with file and line.
5. **Toolchain Universal Mobile Runtime: Connect Copilot** gives VS Code's agent mode the runtime's tools.

From a terminal instead:

```powershell
tumr trust .
tumr run
```

## Tests

`app/src/test/kotlin/.../CalculatorTest.kt` covers the rules (`gradlew test`).

The calculator rules live in `Calculator.kt`; the screen is `MainActivity.kt`.
