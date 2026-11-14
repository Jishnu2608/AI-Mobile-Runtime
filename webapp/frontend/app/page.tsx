import { Hero } from "@/components/home/Hero";
import {
  Agents,
  ArchitectureSection,
  Close,
  Faq,
  Frameworks,
  HowItWorks,
  OpenSource,
  Pillars,
  Privacy,
  RecordingSlot,
} from "@/components/home/Sections";
import { loadTools } from "@/components/docs/McpToolTable";
import { latestStable, loadManifest } from "@/lib/releases";

export default function Home() {
  const tools = loadTools().map((t) => ({ name: t.name, annotations: { readOnlyHint: t.annotations.readOnlyHint } }));
  const hasRelease = latestStable(loadManifest()) !== null;
  return (
    <>
      <Hero hasRelease={hasRelease} toolCount={tools.length} />
      <RecordingSlot />
      <Pillars />
      <HowItWorks />
      <Frameworks />
      <Agents tools={tools} />
      <ArchitectureSection />
      <Privacy />
      <OpenSource />
      <Faq />
      <Close hasRelease={hasRelease} />
    </>
  );
}
