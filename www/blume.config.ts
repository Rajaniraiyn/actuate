import { defineConfig } from "blume";

export default defineConfig({
  title: "Actuate",
  description: "Native UI automation for agents and applications. Compose Rust providers or automate through the CLI and persistent sessions.",
  content: { root: "docs" },
  basePath: "/docs",
  logo: { image: "/mark.svg", text: "Actuate", href: "/" },
  github: { owner: "Rajaniraiyn", repo: "actuate", branch: "docs", dir: "www" },
  theme: { accent: "violet", radius: "md", mode: "system" },
  navigation: {
    actions: [{ label: "Guide", href: "/docs" }],
    sidebar: [
      { label: "Start", items: ["index", "installation", "quick-start"] },
      { label: "Build", items: ["build/rust", "build/typescript", "build/python"] },
      { label: "Automate", items: ["automate/commands", "automate/sessions", "automate/targeting", "automate/configuration"] },
      { label: "Connect", items: ["connect/index"] },
      { label: "Platforms", items: ["platforms/index"] },
      { label: "Design", items: ["design/index", "design/cli", "design/language-apis"] },
    ],
  },
  search: { provider: "orama" },
  ai: {
    llmsTxt: { enabled: true, details: "[Actuate home](https://rajaniraiyn.dev/actuate/). Start with the installation and quick-start guides below." },
    mcp: { enabled: false },
    webmcp: false,
  },
  seo: { sitemap: true, robots: true, structuredData: true, og: { enabled: true } },
  deployment: {
    output: "static",
    site: process.env.PAGES_ORIGIN || "https://rajaniraiyn.dev",
    base: process.env.PAGES_BASE_PATH ?? "/actuate",
  },
});
