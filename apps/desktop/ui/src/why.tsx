// One line per wizard stage: why the developer should care about it (shown under each heading and
// as the step bar's tooltip).
import type { Step } from "./api";

export const STEP_WHY: Record<Step, string> = {
  scan: "Finds what this PC already has (JDK, Android SDK, Node, Flutter, virtualization), so nothing is installed twice. It changes nothing.",
  project: "Tells the runtime which app to build: it reads the folder to learn the framework and the exact tool versions your app needs.",
  plan: "Shows only what this app still needs, with download sizes and where it goes, before anything is downloaded.",
  licences: "The Android SDK, the JDK and other tools come with their own terms; you accept them yourself before they are installed.",
  install: "Downloads and verifies each tool into the runtime's own folder; your PATH, JAVA_HOME and Android Studio stay untouched.",
  ready: "Checks that every installed tool actually works, so a later build cannot fail for a toolchain reason.",
  run: "Boots the emulator, builds and launches your app and opens the live preview: the proof that everything works end to end.",
  agents: "Gives your coding agent the same screen, errors and controls you have, so it can check its own changes.",
};

export function Why({ step }: { step: Step }) {
  return <p className="why">{STEP_WHY[step]}</p>;
}
