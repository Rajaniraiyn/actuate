# Actuate website

The documentation uses [Blume](https://useblume.dev/), pinned in
`bun.lock`. Use Bun 1.4.0 or newer for both package management and the runtime.

```sh
cd www
bun install --frozen-lockfile
bun run dev
```

Before publishing:

```sh
bun run validate
bun run build
bun run audit
bun run preview
```

Write Markdown in `docs/`. `docs/index.md` is the site's welcome page.
Blume generates the layout, navigation, search, and page metadata.
Keep implementation research and validation records in `../_specs/`.
Generated files stay in `.blume/` and `dist/`; do not commit them.

GitHub Actions builds and deploys `dist/` to GitHub Pages. Pull requests validate
and build without deploying. The workflow publishes `docs` during development
and `main` once the docs branch is merged. The deployment origin and base path
come from GitHub Pages metadata, including account-level custom domains.

The static site includes local search and Markdown exports. Its AI assistant
and MCP server are disabled because GitHub Pages cannot run their server code.
