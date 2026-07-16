---
name: security-scan
effort: high
description: Run a security audit covering OWASP Top 10, secrets detection, dependency vulnerabilities, and input validation. Use before releases or after security-sensitive changes.
user-invocable: true
argument-hint: [optional: file or directory path to scope the scan]
context: fork
agent: security
allowed-tools:
  - Read
  - Glob
  - Grep
  - Write
  - Bash(npm audit*)
  - Bash(pnpm audit*)
  - Bash(yarn audit*)
  - Bash(pip-audit*)
  - Bash(cargo audit*)
  - Bash(git log*)
  - Bash(git ls-files*)
  - Bash(git check-ignore*)
---

# Security Scan

You have been asked to perform a security audit. Follow these steps.

## Step 1: Determine Scope

1. If the user specified a file or directory ($ARGUMENTS), scope Steps 3 and 4 to that path.
2. If no scope was specified, scan the entire codebase.
3. Secrets detection (Step 2) always runs repo-wide and deliberately includes gitignored files. A leaked secret is a finding no matter where it lives.

## Step 2: Secrets Detection

### 2a. Tracked-file hygiene (highest-value check, do this first)

1. Run `git ls-files` and flag any tracked file matching: `.env*`, `*.pem`, `*.key`, `id_rsa*`, `*.p12`, `*.pfx`, `*.keystore`. A committed dotenv or key file is CRITICAL.
2. Run `git check-ignore .env .env.local .env.production` to confirm ignore rules actually cover the dotenv files that exist on disk.

### 2b. Value-shaped patterns (near-zero false positives)

Use Grep with these case-sensitive patterns. Cap output with head_limit to avoid flooding context:

- `AKIA[0-9A-Z]{16}` (AWS access key ID)
- `ghp_[A-Za-z0-9]{36}` and `github_pat_[A-Za-z0-9_]{22,}` (GitHub tokens)
- `sk-ant-[A-Za-z0-9-]{20,}` (Anthropic)
- `sk_live_[A-Za-z0-9]{20,}` and `sk_test_[A-Za-z0-9]{20,}` (Stripe)
- `xox[baprs]-[A-Za-z0-9-]{10,}` (Slack tokens)
- `hooks\.slack\.com/services/` (Slack webhooks)
- `AIza[0-9A-Za-z_-]{35}` (Google API key)
- `eyJ[A-Za-z0-9_-]{20,}\.eyJ` (JWTs)
- `-----BEGIN (RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----`
- `(postgres|postgresql|mysql|mongodb(\+srv)?|redis|amqp)://[^/\s:]+:[^@\s]+@` (credentials in connection strings)

### 2c. Keyword assignments to literals

Grep (case-insensitive): `(api_key|apikey|secret|token|passwd|password|credential)s?\s*[:=]\s*['"][^'"]{8,}`

Do not flag bare keyword mentions (variable names, config keys that read from env). Only assignments to string literals count.

### 2d. Files Grep cannot see

Grep respects `.gitignore`, so explicitly Read `.env`, `.env.local`, `.env.production`, and any other dotenv files found via Glob. A real secret in a gitignored, untracked dotenv file is LOW severity (local-only exposure); the same value tracked in git or embedded in source is CRITICAL.

### 2e. CI/CD configuration

Check `.github/workflows/`, `.gitlab-ci.yml`, `Jenkinsfile`, and similar. Inline literal secrets are findings; references like `${{ secrets.NAME }}` or masked CI variables are fine.

### 2f. Git history

For each confirmed secret, run `git log -S "<distinctive fragment>" --oneline` to determine whether it was ever committed. If it was, remediation is: rotate the credential first, then purge history (git filter-repo or BFG). Deleting the line in a new commit does not un-leak it. Say so explicitly in the finding.

### 2g. Triage

For every hit, verify whether it is a placeholder, example, or test fixture before reporting it as a real credential.

## Step 3: OWASP Top 10 Analysis

The categories below follow the OWASP Top 10 2025 edition (verified current as of mid-2026). If you can verify that a newer edition is current, note any differences in the report.

1. **Broken Access Control:** Endpoints missing authorization middleware, direct object references without ownership checks, path traversal (user input flowing into `path.join`, `open`, `readFile`, or equivalent file operations). This category now includes SSRF: user-controlled URLs reaching `fetch`, `axios`, `requests`, `http.get`, `urllib`, or equivalents without an allowlist; watch for access to internal hosts and cloud metadata endpoints (169.254.169.254).
2. **Security Misconfiguration:** Debug mode in production configs, default credentials, XXE (XML parsers with external entity processing enabled), missing security headers, permissive CORS. Specifically flag wildcard or reflected `Access-Control-Allow-Origin` combined with `Access-Control-Allow-Credentials: true`; that combination is the exploitable one.
3. **Software Supply Chain Failures:** Run the available dependency audits (`npm audit`, `pnpm audit`, or `yarn audit`; `pip-audit`; `cargo audit`). Also check: lockfile present and committed, dependencies pinned, install scripts (`postinstall`) in dependencies, package names that look like typosquats of popular packages.
4. **Cryptographic Failures:** PII in logs, unencrypted sensitive data at rest, missing HTTPS enforcement, passwords hashed with MD5/SHA-1 or stored in plaintext, hardcoded crypto keys or static IVs.
5. **Injection:** String concatenation in SQL queries, shell commands built from user input, template injection, LDAP injection, and XSS (`innerHTML`, `dangerouslySetInnerHTML`, unescaped template variables, `eval()`).
6. **Insecure Design:** Missing rate limiting on auth and other expensive endpoints, missing CSRF protection on state-changing routes.
7. **Authentication Failures:** Hardcoded credentials, weak password validation, session fixation, JWT misconfiguration (accepting `alg: none`, weak HMAC secrets, missing expiry validation).
8. **Software and Data Integrity Failures:** Insecure deserialization (`pickle.loads`, `yaml.load` without SafeLoader, Java `ObjectInputStream`, PHP `unserialize`), prototype pollution (deep-merging user input into objects, unfiltered `__proto__` or `constructor` keys), unsigned update or plugin-loading mechanisms. Note: `JSON.parse` itself is safe; the risk is what the parsed object gets merged into.
9. **Logging and Alerting Failures:** Auth failures, access denials, and data modifications not logged; conversely, secrets or PII written to logs.
10. **Mishandling of Exceptional Conditions:** Stack traces or verbose errors returned to users, empty catch blocks that swallow security failures, fail-open error paths (an auth or validation check that proceeds when its dependency throws), crashes on malformed input.

## Step 4: Input Validation

1. Find all entry points (API routes, form handlers, CLI argument parsers).
2. Check each for input validation and sanitization.
3. Look for unsafe regex patterns vulnerable to ReDoS.
4. Check file upload handlers for missing type/size validation.
5. Look for unvalidated redirects and forwards.

## Step 5: Produce Report

Write the full report to `tasks/security-scan-<YYYY-MM-DD>.md` (use today's date), then return the Summary plus Critical and High findings in your response.

```
# Security Scan Report

## Summary
- Scope: <what was scanned and what was excluded>
- Critical: <count>
- High: <count>
- Medium: <count>
- Low: <count>

## Critical Findings
[CRITICAL] <category>: file:line
  Finding: <description>
  Impact: <what an attacker could do>
  Remediation: <specific fix steps>

## High
(same format)

## Medium
(same format)

## Low
(same format)
```

Rules:
- Never include actual secret values in the report. Redact them, keeping only enough of a prefix to identify the credential type.
- For leaked secrets, remediation always starts with rotation, then history purge if the secret was ever committed.
- Do not commit the report. It maps the codebase's weaknesses and may reference secret locations.
