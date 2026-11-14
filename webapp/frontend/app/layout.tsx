import type { Metadata, Viewport } from "next";
import localFont from "next/font/local";
import "./globals.css";
import { ThemeScript } from "@/components/theme/ThemeScript";
import { SiteHeader } from "@/components/site/SiteHeader";
import { SiteFooter } from "@/components/site/SiteFooter";
import { PendingBanner } from "@/components/site/PendingBanner";
import { site } from "@/lib/site";
import { latestStable, loadManifest } from "@/lib/releases";

// Self-hosted variable fonts (no font CDN, dev_plan §11.1): Geist and Geist Mono, SIL OFL 1.1.
const geistSans = localFont({
  src: "../node_modules/geist/dist/fonts/geist-sans/Geist-Variable.woff2",
  variable: "--font-geist-sans",
  weight: "100 900",
  display: "swap",
});
const geistMono = localFont({
  src: "../node_modules/geist/dist/fonts/geist-mono/GeistMono-Variable.woff2",
  variable: "--font-geist-mono",
  weight: "100 900",
  display: "swap",
});

// No metadataBase, canonical URL or sitemap: the domain is not decided (W2 / DEC-038).
export const metadata: Metadata = {
  title: { default: `${site.name} — Android runtime for AI coding agents`, template: `%s · ${site.name}` },
  description:
    "A local Windows runtime that prepares the Android toolchain, runs the emulator, shows your app beside Claude Code or Codex, and gives the agent the screen, the errors and the controls.",
  applicationName: site.name,
  robots: { index: false, follow: false }, // until the public name and domain are decided (W1, W2)
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f7faf6" },
    { media: "(prefers-color-scheme: dark)", color: "#101b16" },
  ],
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const hasRelease = latestStable(loadManifest()) !== null;
  return (
    <html lang="en" className={`${geistSans.variable} ${geistMono.variable}`} suppressHydrationWarning>
      <head>
        <ThemeScript />
      </head>
      <body className="min-h-dvh flex flex-col">
        <a
          href="#main"
          className="sr-only focus:not-sr-only focus:fixed focus:left-4 focus:top-4 focus:z-[100] focus:rounded-sm focus:bg-highlight focus:px-4 focus:py-2.5 focus:font-semibold focus:text-highlight-ink"
        >
          Skip to content
        </a>
        <PendingBanner />
        <SiteHeader hasRelease={hasRelease} />
        <main id="main" className="flex-1" tabIndex={-1}>
          {children}
        </main>
        <SiteFooter />
      </body>
    </html>
  );
}
