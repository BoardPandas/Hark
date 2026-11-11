<!-- PAGE_ID: {page_id} -->
<details>
<summary>Relevant source files</summary>

- [path/to/file:N-M](path/to/file#LN-LM)

</details>

# Runbook

> **Related Pages**: [Architecture](../core/ARCHITECTURE.md), [Monitoring](MONITORING.md)

---

<!-- BEGIN:AUTOGEN {page_id}_overview -->
## Overview

{1-2 sentence description of where the system runs and what this runbook covers.}

| Service | Platform | Region | Notes |
|---|---|---|---|
| api | {platform} | {region} | {notes} |
| worker | {platform} | {region} | {notes} |

Sources: [infra/](infra/), [.github/workflows/](.github/workflows/)
<!-- END:AUTOGEN {page_id}_overview -->

---

<!-- BEGIN:AUTOGEN {page_id}_deployment -->
## Deployment

### Standard deploy

1. {Step with command} -- `{command}`
2. {Step}
3. {Step}

### Rollback

1. {Step}
2. {Step}

Sources: [.github/workflows/deploy.yml](.github/workflows/deploy.yml)
<!-- END:AUTOGEN {page_id}_deployment -->

---

<!-- BEGIN:AUTOGEN {page_id}_health-checks -->
## Health Checks

| Service | Endpoint | Expected | Frequency |
|---|---|---|---|
| api | `GET /healthz` | `200 OK` | {n}s |
| worker | (internal heartbeat) | log within {n}s | continuous |

Sources: [src/routes/healthz.ts:N-M](src/routes/healthz.ts#LN-LM)
<!-- END:AUTOGEN {page_id}_health-checks -->

---

<!-- BEGIN:AUTOGEN {page_id}_scaling -->
## Scaling Procedures

| Trigger | Action |
|---|---|
| Sustained CPU > 70% on api | Increase replicas to N |
| Queue depth > 10k | Increase worker replicas |

Sources: {citations or "_TBD_"}
<!-- END:AUTOGEN {page_id}_scaling -->

---

<!-- BEGIN:AUTOGEN {page_id}_oncall -->
## On-call Procedures

### Paging

- {Who gets paged for what}
- {Escalation policy}

### First-response checklist

1. Check status dashboard at {url}
2. Check recent deploys: `git log --oneline -20 origin/main`
3. Check error rate in {monitoring tool}

Sources: {citations}
<!-- END:AUTOGEN {page_id}_oncall -->

---

<!-- BEGIN:AUTOGEN {page_id}_incidents -->
## Common Incidents

### {Incident: brief title}

**Symptom:** {what users see}

**Diagnosis:**
1. {step}

**Mitigation:**
1. {step}

Sources: {citations or post-mortem links}
<!-- END:AUTOGEN {page_id}_incidents -->

---

<!-- BEGIN:AUTOGEN {page_id}_recovery -->
## Disaster Recovery

- **Database backup cadence:** {cadence}
- **Backup location:** {location}
- **Restore procedure:** {steps or link}

Sources: {citations}
<!-- END:AUTOGEN {page_id}_recovery -->

---
