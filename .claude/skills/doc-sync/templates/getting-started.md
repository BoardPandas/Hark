<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

- [README.md](README.md)
- [package.json](package.json)
- [docker-compose.yml](docker-compose.yml)
- [.env.example](.env.example)

</details>

# Getting Started

> **Related Pages**: [Overview](OVERVIEW.md), [Architecture](core/ARCHITECTURE.md), [Configuration](core/CONFIGURATION.md)

---

<!-- BEGIN:AUTOGEN {page_id}_prerequisites -->
## Prerequisites

| Tool | Version | Why |
|---|---|---|
| Node.js | `{version}` | Runtime |
| {package manager} | `{version}` | Install dependencies |
| Docker | `{version}` | Local Postgres + Redis |
| {other} | `{version}` | {why} |

Sources: [package.json:N](package.json#LN), [docker-compose.yml:1-N](docker-compose.yml#L1-LN)
<!-- END:AUTOGEN {page_id}_prerequisites -->

---

<!-- BEGIN:AUTOGEN {page_id}_install -->
## Installation

```bash
git clone {repo_url}
cd {repo_dir}
{package manager} install
```

Sources: [README.md:N-M](README.md#LN-LM)
<!-- END:AUTOGEN {page_id}_install -->

---

<!-- BEGIN:AUTOGEN {page_id}_environment -->
## Environment Setup

```bash
cp .env.example .env
# edit .env to fill in secrets
```

| Variable | Required | Source |
|---|---|---|
| `{VAR}` | Yes | [.env.example:N](.env.example#LN) |

Sources: [.env.example](.env.example)
<!-- END:AUTOGEN {page_id}_environment -->

---

<!-- BEGIN:AUTOGEN {page_id}_local-services -->
## Local Services

```bash
docker compose up -d
```

| Service | Port | Image |
|---|---|---|
| postgres | 5432 | {image} |
| redis | 6379 | {image} |

Sources: [docker-compose.yml:1-N](docker-compose.yml#L1-LN)
<!-- END:AUTOGEN {page_id}_local-services -->

---

<!-- BEGIN:AUTOGEN {page_id}_run -->
## Run

| Goal | Command |
|---|---|
| Dev server | `{command}` |
| Build | `{command}` |
| Tests | `{command}` |
| Lint | `{command}` |

Sources: [package.json:N-M](package.json#LN-LM)
<!-- END:AUTOGEN {page_id}_run -->

---

<!-- BEGIN:AUTOGEN {page_id}_quick-start -->
## Quick Start

```bash
{step 1}
{step 2}
{step 3}
```

Visit {url} to see the app running.

Sources: [README.md:N-M](README.md#LN-LM)
<!-- END:AUTOGEN {page_id}_quick-start -->

---

<!-- BEGIN:AUTOGEN {page_id}_troubleshooting -->
## Troubleshooting

| Symptom | Cause | Fix |
|---|---|---|
| {symptom} | {cause} | {fix} |

Sources: {citations or "_TBD_"}
<!-- END:AUTOGEN {page_id}_troubleshooting -->

---
