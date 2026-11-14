import type { NextConfig } from "next";

// Static-first (dev_plan §11.1): the site builds to plain files in `out/` that any static host
// can serve. Hosting is still an open decision (W5 / DEC-040), so nothing host-specific lives here.
// Cache Components and partial prefetching need a Node server and are therefore not enabled.
const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  images: { unoptimized: true },
  poweredByHeader: false,
  turbopack: {
    rules: {
      "*.css": {
        loaders: ["@tailwindcss/turbopack"],
        as: "*.css",
      },
    },
  },
};

export default nextConfig;
