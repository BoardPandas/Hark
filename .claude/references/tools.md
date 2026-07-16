# CLI Tools Reference

Claude Code reads this file to know which CLI tools are available and how to use them. When a command fails because a tool is missing, check this file for the install command and offer to install it.

## How This File Works

- **init-repo** and **plan-repo** populate this file based on the detected/chosen stack.
- Each tool entry includes: install command, version check, and common usage.
- Claude Code should check `<tool> --version` before assuming a tool is available.
- If a tool is missing and needed, ask the user before installing.

## Important: No Local Infrastructure

All databases, caches, and backend services run on **Northflank** and **Cloudflare** -- never locally. There is no Docker, no local Postgres, no local Redis. Development connects to remote services via environment variables or Northflank CLI port-forwarding.

## Universal Tools

### Git
- **Check:** `git --version`
- **Usage:** Version control. Always available.

### Node.js / npm (if JS/TS project)
- **Check:** `node --version && npm --version`
- **Install:** https://nodejs.org or `nvm install --lts`
- **Usage:** `npm install`, `npm run <script>`, `npx <command>`

### Bun (alternative JS runtime)
- **Check:** `bun --version`
- **Install:** `curl -fsSL https://bun.sh/install | bash`
- **Usage:** `bun install`, `bun run <script>`, `bunx <command>`

### Python / pip (if Python project)
- **Check:** `python3 --version && pip3 --version`
- **Install:** https://python.org or system package manager
- **Usage:** `pip install -r requirements.txt`, `python3 -m <module>`

## Stack-Specific Tools

This section is populated by plan-repo or init-repo based on the project's stack. Below are common examples.

### Package Managers
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| pnpm | `pnpm --version` | `npm i -g pnpm` | Monorepos, disk-efficient deps |
| yarn | `yarn --version` | `npm i -g yarn` | Projects using yarn.lock |
| uv | `uv --version` | `pip install uv` | Fast Python package management |
| cargo | `cargo --version` | https://rustup.rs | Rust projects |
| go | `go version` | https://go.dev/dl | Go projects |

### Linters & Formatters
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| biome | `npx biome --version` | `npm i -D @biomejs/biome` | Recommended default for new JS/TS projects (BP): single Rust-based lint + format tool replacing ESLint+Prettier |
| eslint | `npx eslint --version` | `npm i -D eslint` | Mature codebases needing its plugin ecosystem (React Hooks, a11y, security); consider biome or oxlint for greenfield/speed-sensitive projects |
| oxlint | `npx oxlint --version` | `npm i -D oxlint` | Fast drop-in ESLint rule coverage; pair with eslint for plugin-only rules |
| prettier | `npx prettier --version` | `npm i -D prettier` | JS/TS/CSS/MD formatting (increasingly folded into biome) |
| ruff | `ruff --version` | `pip install ruff` | Fast Python lint + format |
| rustfmt | `rustfmt --version` | Included with rustup | Rust formatting |

### Test Runners
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| vitest | `npx vitest --version` | `npm i -D vitest` | Vite-based JS/TS projects |
| jest | `npx jest --version` | `npm i -D jest` | JS/TS testing |
| pytest | `pytest --version` | `pip install pytest` | Python testing |
| playwright | `npx playwright --version` | `npm i -D @playwright/test` | E2E browser testing |

### Build Tools
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| vite | `npx vite --version` | `npm i -D vite` | Frontend builds (Vite 8+ bundles via Rolldown internally) |
| turbo | `npx turbo --version` | `npm i -D turbo` | Monorepo builds |
| rolldown | `npx rolldown --version` | `npm i -D rolldown` | Rust bundler; default inside Vite 8+ (no separate install for Vite users), standalone for library bundling |
| esbuild | `npx esbuild --version` | `npm i -D esbuild` | Fast JS bundling (standalone or via tsup; superseded inside Vite by Rolldown) |
| tsc | `npx tsc --version` | `npm i -D typescript` | TypeScript compilation |

### Database Tools (Remote Only)
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| prisma | `npx prisma --version` | `npm i -D prisma` | Prisma ORM v7+ (pure TS/WASM client, native edge-runtime support; connects to Northflank Postgres) |
| drizzle-kit | `npx drizzle-kit --version` | `npm i -D drizzle-kit` | Drizzle ORM (connects to Northflank Postgres) |

### Deployment Tools
| Tool | Check | Install | Use When |
|------|-------|---------|----------|
| wrangler | `npx wrangler --version` | `npm i -D wrangler` | Cloudflare R2 & DNS/CDN (not Pages -- frontend is on Northflank) |
| northflank | `northflank --version` | `npm i -g @northflank/cli` | Northflank frontend, backend, Postgres, Redis |

## Available MCP Servers

Claude Code has access to the following MCP (Model Context Protocol) servers. These provide direct integration with external services without needing CLI tools.

### Cloudflare MCPs

| MCP Server | Purpose |
|------------|---------|
| **cloudflare-observability** | Query Worker logs, inspect structured log payloads, list Workers |
| **cloudflare-workers-builds** | View and debug Workers Builds CI/CD (list builds, get build logs) |
| **cloudflare-workers-bindings** | Manage KV, R2, D1, Hyperdrive bindings; read Worker code |
| **cloudflare-containers** | Sandboxed Ubuntu container for running commands, reading/writing files |
| **cloudflare-radar** | Global internet insights: traffic, attacks, rankings, BGP, URL scanning |
| **cloudflare-docs** | Search Cloudflare documentation, Pages-to-Workers migration guides |
| **cloudflare-api** | Execute and search raw Cloudflare API endpoints |
| **cloudflare-graphql** | Query Cloudflare's GraphQL analytics API, explore schema |
| **cloudflare-dns-analytics** | DNS analytics reports, zone and account DNS settings |
| **cloudflare-audit-logs** | Query account audit logs |
| **cloudflare-logpush** | Manage Logpush jobs by account |
| **cloudflare-browser** | Fetch URL content as HTML, Markdown, or screenshot |
| **cloudflare-ai-gateway** | Inspect AI Gateway logs (request/response bodies) |
| **cloudflare-ai-search** | Search using Cloudflare AI Search / RAG |
| **cloudflare-casb** | Query CASB integrations, assets, and categories |
| **cloudflare-dex** | Digital Experience monitoring: fleet status, HTTP/traceroute tests, WARP diagnostics |
| **cloudflare-agents-sdk-docs** | Search Cloudflare Agents SDK documentation |

### GitHub MCP

| MCP Server | Purpose |
|------------|---------|
| **github** | Full GitHub integration: repos, issues, PRs, branches, commits, code search, releases, reviews |

### Communication & Productivity MCPs

| MCP Server | Purpose |
|------------|---------|
| **claude_ai_Slack** | Read/search channels, send messages, create canvases, search users |
| **claude_ai_Gmail** | Search/read emails, create drafts, get profile |
| **claude_ai_Google_Calendar** | List/create/update events, find free time, RSVP |
| **claude_ai_Notion** | Search/create/update pages and databases, query views, manage comments |

### Notion MCP (Direct)

| MCP Server | Purpose |
|------------|---------|
| **notion** | Direct Notion API: search, create/update pages, query databases, manage comments |

### Infrastructure MCPs

| MCP Server | Purpose |
|------------|---------|
| **northflank** | Full Northflank management: projects, services, addons (Postgres, Redis), jobs, secrets, volumes, domains, templates, builds, metrics |
| **railway** | Railway platform: projects, services, deployments, variables, logs, domains |
| **doppler** | Secrets management: projects, configs, secrets, environments, integrations, service accounts |

### Browser Automation MCPs

| MCP Server | Purpose |
|------------|---------|
| **claude-in-chrome** | Chrome browser automation: navigate, click, type, read pages, take screenshots, record GIFs, execute JS |
| **playwright** | Playwright browser automation: navigate, click, fill forms, take screenshots, evaluate JS, handle dialogs |

### Analytics and Product MCPs

| MCP Server | Purpose |
|------------|---------|
| **posthog** | Product analytics: feature flags, experiments, insights, cohorts, dashboards, events, surveys |
| **asana** | Project management: tasks, projects, goals, portfolios, teams, attachments |

### Meetings and Productivity MCPs

| MCP Server | Purpose |
|------------|---------|
| **krisp** | Meeting transcripts, action items, activities, upcoming meetings, user preferences |

### IT Management MCPs

| MCP Server | Purpose |
|------------|---------|
| **ninjaone** | RMM/endpoint management: devices, organizations, tickets, patches, scripts, alerts |
| **zendesk** | Help desk: tickets, users, organizations, triggers, automations, macros, views |

## Project-Specific Tools

<!-- init-repo and plan-repo append project-specific entries here -->
<!-- Format: ### Tool Name -->
<!-- - **Check:** `command --version` -->
<!-- - **Install:** `install command` -->
<!-- - **Usage:** Common commands for this project -->
