# CLI Tools Reference

Claude Code reads this file to know which CLI tools are available and how to use them. When a command fails because a tool is missing, check this file for the install command and offer to install it.

## How This File Works

- **init-repo** and **plan-repo** populate this file based on the detected/chosen stack.
- Each tool entry includes: install command, version check, and common usage.
- Claude Code should check `<tool> --version` before assuming a tool is available.
- If a tool is missing and needed, ask the user before installing.

## Important: This Is a Native Desktop App

Hark is an offline, single-user **Rust desktop application** for Windows + macOS. There is **no web infrastructure** — no Northflank, no Cloudflare, no Postgres/Redis, no Docker, no local server. The only storage is a local SQLite file; the only network call is the user's optional BYOK LLM endpoint. Do **not** add web/hosting tooling (wrangler, northflank CLI, npm/pnpm bundlers, ORMs) to this project.

Note: this machine is coding-only. Build/test/lint/typecheck here; run and validate the running app (mic, hotkey, injection, notarization) on real macOS and Windows.

## Universal Tools

### Git
- **Check:** `git --version`
- **Usage:** Version control. Always available.

### Rust toolchain (rustup)
- **Check:** `rustup --version && rustc --version`
- **Install:** https://rustup.rs
- **Usage:** `rustup update stable`, `rustup target add <triple>` for cross-compilation.

## Rust Desktop Tools

### Build, run, format, lint
| Tool | Check | Install | Usage |
|------|-------|---------|-------|
| cargo | `cargo --version` | via rustup | `cargo build`, `cargo run`, `cargo build --release` |
| rustfmt | `cargo fmt --version` | `rustup component add rustfmt` | `cargo fmt` (format), `cargo fmt --check` (CI) |
| clippy | `cargo clippy --version` | `rustup component add clippy` | `cargo clippy --all-targets -- -D warnings` |
| cargo-nextest | `cargo nextest --version` | `cargo install cargo-nextest` | `cargo nextest run` (faster test runner; `cargo test` also fine) |

### Cross-compilation & CI
| Tool | Check | Install | Usage |
|------|-------|---------|-------|
| rustup targets | `rustup target list --installed` | `rustup target add x86_64-pc-windows-msvc aarch64-apple-darwin x86_64-apple-darwin` | Build for both OSes/arches |
| cargo-audit | `cargo audit --version` | `cargo install cargo-audit` | `cargo audit` — dependency vulnerability scan |

### Packaging & signing (Phase 5)
| Tool | Check | Install | Usage |
|------|-------|---------|-------|
| cargo-dist | `cargo dist --version` | `cargo install cargo-dist` | Cross-platform installer/artifact generation |
| cargo-bundle | `cargo bundle --version` | `cargo install cargo-bundle` | Build `.app` / platform bundles (alternative to cargo-dist) |
| codesign / notarytool | `xcrun notarytool --help` | Xcode command-line tools (macOS) | Sign + notarize the macOS build |
| WiX / signtool | `signtool` (VS tools) | Windows SDK / WiX Toolset | MSI packaging + Authenticode signing |

### Key native dependencies (not CLI tools — crates + assets)
- **sherpa-onnx** crate (v1.13.4+) bundles/links **ONNX Runtime**. Verify CoreML (macOS) / DirectML (Windows) cargo feature flags in the crate source before enabling GPU inference.
- **Parakeet TDT 0.6B v2 (English) ONNX** model files — place in `models/` (bundled at package time or fetched on first run). Keep out of git.

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
