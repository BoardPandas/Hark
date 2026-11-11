# Research Waves, Subagent Prompts & Stack Recommendation Template

Referenced by `plan-repo` Step 4 and Step 5. Read this file before dispatching Step 4 research waves or formatting the Step 5 recommendation.

## Step 4: Research Subagent Prompts & Seed Candidates

Remember: write the literal `<DATE>` (YYYY-MM-DD) into every subagent prompt, pass relevant Step 3 LL-G selection-input gotchas, and treat seed lists as seeds rather than closed menus.

### Wave 1: Platform Profile Subagent (runs whenever a backend is in scope)

Read `.claude/references/infrastructure.md` before writing this prompt and carry its constraints into the comparison — particularly that Cloudflare has no Redis, D1 caps at 10 GB, and Workers cannot run long-lived processes or native npm bindings.

Prompt:
"Compare two hosting profiles for [one-line project description] as of <DATE>: (A) Cloudflare -- Workers for frontend and API, D1 or Hyperdrive-fronted external Postgres, Workers KV and Durable Objects, Cloudflare Queues, Cron Triggers, native CDN/WAF; (B) Railway -- long-running container services for frontend and API, managed Railway Postgres and Redis, cron service type, persistent volumes, private networking, with a Cloudflare proxy in front for CDN/WAF. Both profiles use Cloudflare R2 for object storage, Resend for email, and Better Auth inside the API. The project's requirements are: [paste the Step 2 answers -- scale, data, real-time needs, integrations, and for desktop projects what the server is actually for]. Compare on: fit for these specific workloads, database semantics needed (SQLite via D1 versus real Postgres with extensions), background and long-running work, websocket support, cost shape at this traffic level, operational complexity, and Better Auth support on each. Report a recommendation, the single strongest argument against it, and any requirement that rules one profile out. WHY: we deploy on exactly one of these two profiles and need the one that fits this workload; the choice then constrains the language, framework, and ORM picks that follow."

### Wave 1: Desktop Framework Subagent (runs for desktop or both)

Prompt:
"Research the current best frameworks for building an installable desktop application as of <DATE>, targeting [operating systems from question 10]. The app is: [one-line description]. It needs: [OS capabilities from question 12], [update mechanism from question 13], and [server needs from question 11, or 'no server']. Compare: maturity and maintenance activity, bundle size and memory footprint, rendering approach (system webview versus bundled Chromium versus own engine), access to native OS APIs, code signing and notarization story per OS, auto-update support, mobile reach if that matters later, hiring and ecosystem depth, and how well the team's likely existing skills transfer. Report a recommendation with a runner-up and the trade-off that separates them. WHY: this choice fixes the client language, the UI toolkit, and the packaging pipeline for the whole project, so it has to be made before anything else."

Seed candidates:
- **Web-tech shells:** Tauri (Rust core, system webview) vs Electron (bundled Chromium/Node) vs Wails (Go core, system webview) vs Neutralino
- **Own-renderer toolkits:** Flutter desktop (Dart/Skia) vs Compose Multiplatform (Kotlin) vs Avalonia or .NET MAUI (C#) vs Qt (C++/Python)
- **Rust-native UI:** egui vs Slint vs Dioxus desktop
- **Fully native:** Swift/SwiftUI on macOS, WinUI 3 on Windows, GTK on Linux — worth recommending when only one OS matters and native feel is the product
- Key factors: system webview vs pixel-identical rendering across OSes; whether required capabilities (USB/serial, GPU, background daemon, bundled binaries) rule an option out; signing and notarization burden per target OS.

### Wave 2: Language & Runtime Subagent (runs whenever a backend is in scope)

Prompt:
"Research the current state of [relevant languages] for the server side of an application deployed on the [chosen profile] profile, as of <DATE>. [For Cloudflare: the API runs as a Cloudflare Worker on V8 isolates with nodejs_compat, so runtimes and native npm bindings that cannot run there are disqualified. For Railway: the API runs as a long-running container, so any runtime that containerizes cleanly is eligible.] The backend must support Better Auth and connect to [D1 or Hyperdrive-fronted Postgres | Railway Postgres and Redis]. Compare: ecosystem maturity, Better Auth SDK support, [Workers runtime compatibility | container build and cold-start quality], and developer tooling quality. WHY: we need the best server language that actually runs on the profile we have chosen."

Seed candidates:
- **TypeScript (Node)** vs **TypeScript (Bun)** vs **Go** vs **Rust** vs **Python** vs **Elixir**
- On Cloudflare, Workers run JavaScript/TypeScript and WASM; non-JS languages require Cloudflare Containers, which is a reason to reconsider the profile rather than force the language. Check Better Auth official SDK support.

### Wave 2: Frontend Framework Subagent (runs for web or both)

Prompt:
"Research the current best frontend frameworks for deployment on the [chosen profile] profile as of <DATE>. [For Cloudflare: the frontend ships as a Worker with Static Assets -- either a prebuilt SPA or SSR through the framework's Cloudflare adapter. Pages is legacy; do not recommend it. Report which frameworks have a first-party, actively maintained Workers adapter. For Railway: the frontend runs as a container, either static-served or an SSR Node/Bun server.] Compare: quality of the deployment path on that profile, SPA versus SSR versus hybrid trade-offs, build speed, ecosystem size, Better Auth client SDK support, and developer experience. WHY: the frontend has to deploy cleanly on the profile we have chosen and integrate with Better Auth client-side."

Seed candidates:
- **Next.js** vs **SvelteKit** vs **Nuxt** vs **Astro** vs **React Router v7 (formerly Remix)** vs **Solid Start** vs **React (SPA with Vite)**
- Key factor on Cloudflare: adapter quality and whether SSR features survive Workers. On Railway: containerization cleanliness. For "both" targets: component sharing with desktop UI.

### Wave 2: Desktop UI & Local Data Subagent (runs for desktop or both)

Prompt:
"Research the current best UI approach and local data layer for a [chosen desktop framework] application as of <DATE>, on [target operating systems]. The app is: [one-line description]; its UI is [answer to question 5]. Cover: which UI toolkit or component library suits this framework, how to look native on each target OS without maintaining three UIs, the recommended local persistence layer (embedded SQLite and its bindings, key-value stores, or plain files) with migration tooling, secure storage of secrets and tokens in the OS keychain, and -- if the app has a server -- the offline-first sync pattern and conflict-resolution approach that fits. WHY: desktop users judge an app on native feel and on not losing their data offline, so the UI toolkit and the local store are the two decisions that shape the rest of the client."

Seed candidates:
- **Web-tech shells:** web UI libraries (shadcn/ui, Mantine, Park UI) plus desktop kits; native window chrome vs custom titlebars
- **Local data:** SQLite (`tauri-plugin-sql`, `better-sqlite3`, `sqlx`), SQLite + sync layer, embedded KV stores, or JSON/TOML config files
- **Secrets:** OS keychain integration (`keytar`, `tauri-plugin-stronghold`, `keyring`), never plaintext on disk
- **Sync (if backend in scope):** last-write-wins vs CRDTs (Yjs, Automerge) vs server-authoritative model

### Wave 3: Backend Framework Subagent (runs whenever a backend is in scope)

Prompt:
"Research the current best backend/API frameworks for [recommended language] on the [chosen profile] profile, as of <DATE>. The framework must support Better Auth middleware, connect to [the profile's database], and use the Cloudflare R2 S3-compatible API. [For Cloudflare: it must run on Workers -- report first-party Workers support, not just 'works in Node'. For Railway: it runs in a long-running container alongside Redis.] Compare: performance, middleware ecosystem, Better Auth integration quality, and [cold-start behaviour on Workers | container startup time]. WHY: the API hosts Better Auth endpoints alongside application logic and has to be a first-class citizen on our chosen profile."

Seed candidates (TypeScript): **Hono** vs **Express** vs **Fastify** vs **Elysia** vs **tRPC** (layer on top). Weight Workers-native support heavily on Cloudflare.

### Wave 3: UI Library Subagent (runs for web or both)

Prompt:
"Research the current best UI component libraries and styling approaches for [frontend framework], as of <DATE>. Compare: component quality, accessibility out-of-box, theming/customization, bundle size, maintenance activity, design system maturity. WHY: We need a UI approach that gives the best developer experience and end-user quality for [project type]."

Seed candidates:
- **Component libraries:** shadcn/ui vs Radix vs Ark UI vs Mantine vs MUI vs Chakra vs Park UI
- **Styling:** Tailwind CSS vs CSS Modules vs vanilla-extract vs Panda CSS vs UnoCSS
- **Animation:** Framer Motion vs Motion One vs GSAP vs CSS-only

### Wave 3: ORM & Data Layer Subagent (runs whenever a backend is in scope)

Prompt:
"Research the current best ORM/query builder options for [recommended language] against [D1 (SQLite) | Postgres through Hyperdrive | Railway Postgres], as of <DATE>. Compare: type safety, migration tooling, query performance, [D1 driver support and Workers compatibility | connection pooling], and Better Auth database adapter support. WHY: the ORM has to work with the database our chosen profile gives us and support Better Auth's adapter."

Seed candidates (TypeScript): **Drizzle** vs **Prisma** vs **Kysely** vs **TypeORM** (check Better Auth database adapter and real D1 driver vs Node-only Postgres driver).

### Wave 3: Packaging & Distribution Subagent (runs for desktop or both)

Prompt:
"Research the current packaging, signing, and update pipeline for a [chosen desktop framework] app on [target operating systems], as of <DATE>. Cover: installer formats per OS (MSI/NSIS, DMG/PKG, AppImage/deb/rpm/Flatpak), code signing requirements and current certificate costs, macOS notarization and stapling, Windows SmartScreen reputation, the recommended auto-update mechanism and how updates are signed and verified, CI that can build and sign for every target OS, and whether app-store distribution changes any of it. WHY: [update mechanism from question 13] -- unsigned or unnotarized desktop builds are blocked outright by modern OSes, so this pipeline has to be planned in Phase 1, not discovered at ship time."

Seed candidates:
- **Builders:** `tauri build`, electron-builder, Wails build, `flutter build`, `dotnet publish`
- **Update mechanisms:** Tauri updater, electron-updater / Squirrel, Sparkle (macOS), MSIX/App Installer (Windows), or manifest-plus-artifacts bucket
- **Artifact hosting:** Cloudflare R2 for update manifests and installers
- **CI:** GitHub Actions matrix builds per target OS runner

### Wave 3: Tooling Subagent (always runs)

Prompt:
"Research the current recommended developer tooling for [recommended language(s) -- include the desktop client language if different from the server], as of <DATE>. Compare: speed, reliability, ecosystem compatibility. WHY: We need to populate tools.md with the fastest and most reliable tools for this stack."

Seed candidates:
- **Package managers:** npm vs pnpm vs yarn vs bun (plus cargo, go mod, uv, etc.)
- **Bundlers:** Vite vs Turbopack vs esbuild vs Rspack
- **Linters:** ESLint vs Biome vs oxc-lint (plus clippy, golangci-lint, ruff)
- **Formatters:** Prettier vs Biome vs dprint
- **Test runners:** Vitest vs Jest vs Bun test vs Playwright vs Cypress (plus desktop E2E drivers)
- **Monorepo (if needed):** Turborepo vs Nx vs moon

## Step 5: Stack Recommendation Markdown Template

Include only the sections that this project's routing actually produced (omit Platform Profile for local-only desktop apps; omit Desktop Client for web-only projects):

```markdown
# Stack Recommendation

**Delivery target:** <web, desktop, or both>
**Backend in scope:** <yes -- for X and Y, or no, fully local>

## Platform Profile
**Recommended:** <Cloudflare, or Railway>
**Why:** <2-3 sentences tied to this project's actual requirements>
**Strongest argument against:** <the real trade-off being accepted>
**Runner-up:** <the other profile> (<what would flip the decision>)

| Layer | Choice |
|-------|--------|
| Frontend hosting | <Workers with Static Assets, or Railway container service> |
| Backend hosting | <Workers, or Railway container service> |
| Database | <D1, or Postgres via Hyperdrive, or Railway Postgres> |
| Cache / state | <Workers KV + Durable Objects, or Railway Redis> |
| Queues | <Cloudflare Queues, or Redis queue / worker service> |
| Cron | <Cron Triggers, or Railway cron service> |
| Object storage | Cloudflare R2 |
| Auth | Better Auth |
| Email | Resend |
| CDN | <native, or Cloudflare proxy in front of Railway> |

## Desktop Client
**Framework:** <choice> <version>
**Why:** <2-3 sentences, must reference the target OSes and the required OS capabilities>
**Runner-up:** <choice> (<trade-off>)
**Client language:** <language> (<note if it differs from the server language and why that is fine>)
**Local data:** <store> (<migrations, and where secrets live>)
**Sync model:** <offline-first strategy, or "n/a -- no server">
**Packaging:** <installer formats per OS>
**Signing:** <per-OS requirements and what has to be bought or provisioned>
**Updates:** <mechanism, and where artifacts are hosted>

## Language & Runtime
**Recommended:** <choice> <version>
**Why:** <2-3 sentences specific to this project + the chosen profile>
**Runner-up:** <choice> (<why it lost>)

## Frontend Framework
**Recommended:** <choice> <version>
**Serving mode:** <SPA (static assets), or SSR (adapter or server container)>
**Why:** <2-3 sentences, must reference the chosen profile's deployment path and justify the serving mode>
**Runner-up:** <choice> (<trade-off>)

## Backend Framework
**Recommended:** <choice> <version>
**Why:** <2-3 sentences, must reference the chosen profile + Better Auth>
**Runner-up:** <choice> (<trade-off>)

## UI Approach
**Component library:** <choice> (<why>)
**Styling:** <choice> (<why>)
**Rationale:** <how these choices work together; for "both" targets, whether the web and desktop UIs share components>

## ORM / Data Layer
**ORM:** <choice> (<why, must reference both the profile's database and Better Auth adapter support>)

## Developer Tooling
**Package manager:** <choice>
**Bundler:** <choice>
**Linter + Formatter:** <choice>
**Test runner:** <choice>

## Full Stack Summary
| Layer | Choice | Version | Why |
|-------|--------|---------|-----|
| Platform profile | ... | n/a | ... |
| Language | ... | ... | ... |
| Frontend framework | ... | ... | ... |
| Frontend serving mode | SPA or SSR | n/a | ... |
| Backend framework | ... | ... | ... |
| Desktop framework | ... | ... | ... |
| Local data store | ... | ... | ... |
| UI library | ... | ... | ... |
| Styling | ... | ... | ... |
| ORM | ... | ... | ... |
| Package mgr | ... | ... | ... |
| Linter | ... | ... | ... |
| Test runner | ... | ... | ... |
```
