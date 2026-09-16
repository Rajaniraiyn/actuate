import { defineConfig } from "blume";

export default defineConfig({
  title: "Actuate",
  description: "Native UI automation for agents and applications. Compose Rust providers or automate through the CLI and persistent sessions.",
  content: { root: "docs" },
  logo: { text: "Actuate", href: "/" },
  github: { owner: "Rajaniraiyn", repo: "actuate", branch: "main", dir: "www" },
  theme: { accent: "violet", radius: "md", mode: "system" },
  navigation: {
    sidebar: [
      { label: "Start", items: ["index", "installation", "quick-start"] },
      { label: "Build", items: ["build/rust", "build/typescript", "build/python"] },
      { label: "Automate", items: ["automate/commands", "automate/sessions", "automate/targeting", "automate/configuration"] },
      { label: "Guides", items: ["guides/snapshots", "guides/input-and-cursors", "guides/files-and-dialogs", "guides/errors"] },
      { label: "Connect", items: ["connect/index"] },
      { label: "Platforms", items: ["platforms/index"] },
      { label: "Developers", items: ["developers/index"] },
    ],
  },
  search: { provider: "orama" },
  ai: {
    llmsTxt: true,
    mcp: { enabled: false },
    webmcp: false,
  },
  seo: { sitemap: true, robots: true, structuredData: true, og: { enabled: true } },
  deployment: {
    output: "static",
    site: (process.env.PAGES_ORIGIN || "https://rajaniraiyn.dev").replace(/^http:/, "https:"),
    base: process.env.PAGES_BASE_PATH ?? "/actuate",
  },
});
