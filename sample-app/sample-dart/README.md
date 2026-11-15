# Calculator — Dart (Flutter)

A small calculator for Android written in **Dart with Flutter**, used to try the AI Mobile Runtime: run it on the emulator, edit it, break it on purpose and watch the runtime (and your coding agent) report the problem.

What it does: digits, `.`, `+ − × ÷`, `=`, `AC`, `⌫`, `±` and `%`, with the pending calculation shown above the result. Dividing by zero shows `Error`.

## Run it

In VS Code with the **AI Mobile Runtime** extension (the workspace recommends it):

1. Open this folder (`sample-app/sample-dart`).
2. Run **AI Mobile Runtime: Run on Emulator** from the Command Palette, or click **Mobile** in the status bar (bottom left) for all actions. The first time it asks to trust the folder, because building runs the project's own scripts.
3. The phone appears in the **Mobile Preview** panel. Save a file and the app updates: Flutter hot-reloads saved Dart files in about a second, keeping the calculator's state.
4. Build errors and crashes appear in VS Code's **Problems** panel with file and line.
5. **AI Mobile Runtime: Connect Copilot** gives VS Code's agent mode the runtime's tools.

From a terminal instead:

```powershell
umr trust .
umr run
```

## Tests

`test/calculator_test.dart` covers the rules (`flutter test`).

The calculator rules live in `lib/calculator.dart`; the screen is `lib/main.dart`.
