/** The real loop, step by step. Every command and tool here exists in the product. */
export type Part = "project" | "toolchain" | "core" | "emulator" | "agent" | "preview" | "problems";

export type Stage = {
  id: string;
  title: string;
  body: string;
  command: string;
  who: "you" | "agent";
  parts: Part[];
};

export const stages: Stage[] = [
  {
    id: "open",
    title: "Open your project",
    body: "Point the runtime at a Kotlin, Java, Flutter, React Native or Expo folder. It names the framework and the files that show it, and builds nothing until you trust the folder.",
    command: "umr project .\numr trust .",
    who: "you",
    parts: ["project", "core"],
  },
  {
    id: "detect",
    title: "Detect the environment",
    body: "It reads the project's own versions — Gradle wrapper, plugin, Flutter and Node constraints — compares them with your PC and lists only what is missing, with sizes and licences.",
    command: "umr plan .\numr install .",
    who: "you",
    parts: ["project", "toolchain", "core"],
  },
  {
    id: "emulator",
    title: "Start the emulator",
    body: "A phone of the size you pick boots headless from a saved snapshot, on the runtime's own adb server, so Android Studio keeps working.",
    command: "umr device use standard",
    who: "you",
    parts: ["core", "emulator"],
  },
  {
    id: "run",
    title: "Run the app",
    body: "The framework's adapter builds, installs and launches the app. Saved files then reload by themselves: hot reload for Flutter, Fast Refresh for React Native.",
    command: "umr run",
    who: "you",
    parts: ["project", "core", "emulator", "preview"],
  },
  {
    id: "agent",
    title: "Connect your coding agent",
    body: "One command writes the agent's MCP entry and short instructions, after showing you the exact change. The agent gets its own token and limited permissions.",
    command: "umr agent setup claude-code",
    who: "you",
    parts: ["agent", "core"],
  },
  {
    id: "preview",
    title: "Watch the live preview",
    body: "The app runs in a window docked beside your agent. You click and type in it; the agent sees the same screen through screenshots and the UI tree.",
    command: "screenshot · ui_tree · tap",
    who: "agent",
    parts: ["agent", "core", "emulator", "preview"],
  },
  {
    id: "errors",
    title: "Catch the errors",
    body: "Build errors, crashes and runtime exceptions arrive classified, deduplicated and located at file and line, marked as your code or the environment.",
    command: "runtime_status · get_diagnostic",
    who: "agent",
    parts: ["emulator", "core", "problems", "agent"],
  },
  {
    id: "fix",
    title: "Fix and verify",
    body: "The agent edits the code, the app reloads, and the next status shows no new problems. The agent taps through the screen to check the fix.",
    command: "reload · runtime_status · screenshot",
    who: "agent",
    parts: ["agent", "project", "core", "emulator", "preview"],
  },
];
