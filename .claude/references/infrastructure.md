# Infrastructure Stack (Fixed)

This is the standard hosting architecture for all projects bootstrapped from this template. The plan-repo skill must use this infrastructure -- it is not negotiable or user-configurable.

## Architecture

```
User Browser
    |
    +---> Cloudflare edge (DNS + orange-cloud proxy: CDN, WAF, DDoS, TLS)
              |
              +---> Northflank frontend service (SPA or SSR container)
              |         |
              |         +---> Northflank API (all backend logic + auth)
              |
              +---> Cloudflare R2 (public file downloads, same Cloudflare network)
    |
    +---> Better Auth endpoints (hosted within Northflank API)
              |
              +---> /api/auth/sign-in
              +---> /api/auth/sign-up
              +---> /api/auth/sign-in/social (Google, Microsoft)
              +---> /api/auth/magic-link
              +---> /api/auth/passkey
              +---> /api/auth/two-factor

Northflank API Service
    |
    +---> Northflank Postgres (application data + auth tables)
    +---> Northflank Redis (caching, sessions, job queues)
    +---> Cloudflare R2 (signed URLs for private downloads)
    +---> Email provider (Resend for magic links, OTPs)
    +---> External APIs (Stripe, webhooks, etc.)

Northflank Cron Jobs
    |
    +---> Scheduled tasks (cleanup, reports, sync, email digests)
    +---> Runs as standalone container on schedule
```

## Infrastructure Decisions (Locked)

| Layer | Choice | Notes |
|-------|--------|-------|
| Frontend hosting | Northflank | Container-based. SPA (static-served) or SSR -- per-project choice, decided by plan-repo from the chosen framework |
| Backend hosting | Northflank | Container-based, all API logic and auth |
| Database | Northflank Postgres | Application data + Better Auth tables |
| Cache/Queue | Northflank Redis | Sessions, caching, job queues |
| Object storage | Cloudflare R2 | Public downloads (direct) + private downloads (signed URLs) |
| Auth | Better Auth | Hosted within the Northflank API, not a separate service |
| Cron jobs | Northflank Cron | Standalone container on schedule |
| Email | Resend | Magic links, OTPs, transactional email |
| CDN | Cloudflare proxy (orange-cloud) in front of Northflank | Cloudflare DNS proxies the Northflank frontend origin: CDN, WAF, DDoS, HTTP/3, TLS, Cache Rules. Northflank's built-in Fastly CDN is the simpler no-WAF fallback. |

## CDN Setup Notes (Locked)

The frontend is served from a Northflank container; Cloudflare sits in front as an orange-cloud proxy. Both the frontend and the API run on Northflank -- whether the frontend is a separate Northflank service or combined with the API is a per-project decision (SPA tends toward a static-served service, SSR toward its own service). When wiring the Cloudflare-in-front-of-Northflank CDN:

- **TLS mode must be Full (Strict).** Northflank auto-provisions Let's Encrypt certs. "Flexible" breaks the origin handshake.
- **ACME vs proxy conflict.** Cloudflare proxy must be off (grey-cloud, DNS-only) during Northflank's initial domain verification so the ACME challenge can complete, then flipped to orange. Alternatively, add a Cloudflare WAF bypass rule for `/.well-known/acme-challenge/*` so cert renewals don't require toggling the proxy.
- **HTML is not cached by default.** Fine for an SPA. For SSR, add explicit Cloudflare Cache Rules to cache pages, or send `Cache-Control: no-store` for pages that must not be cached.
- **R2 stays free at the edge.** Because R2 and the CDN are both Cloudflare, edge-to-R2 egress is zero-cost.
- **Fallback:** Northflank's built-in Fastly CDN (per-subdomain toggle) is acceptable when Cloudflare's WAF/DDoS layer is not needed. It has no WAF or bot protection.

## Auth Methods (Better Auth)

All projects include these auth methods by default:
- Email/password sign-in and sign-up
- Social sign-in (Google, Microsoft)
- Magic link
- Passkey (WebAuthn)
- Two-factor authentication (TOTP)

## What plan-repo Still Decides

The infrastructure above is fixed. Plan-repo researches and recommends only:
- Language and runtime (TypeScript/Bun, TypeScript/Node, Go, Rust, etc.)
- Frontend framework (Next.js, SvelteKit, Astro, Nuxt, etc.) -- the framework is chosen, but it is always hosted on Northflank (not Cloudflare Pages). The framework choice drives whether the frontend is a static-served SPA or an SSR container.
- Backend framework (Hono, Express, Fastify, Elysia, etc.)
- UI component library (shadcn/ui, Mantine, MUI, etc.)
- Styling approach (Tailwind, CSS Modules, etc.)
- ORM/query builder (Drizzle, Prisma, Kysely, etc.)
- Developer tooling (package manager, linter, formatter, test runner)
