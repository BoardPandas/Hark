# Releasing Hark

Hark ships as a signed Windows `.exe`, built and published by
[`.github/workflows/release.yml`](workflows/release.yml). Signing uses Azure
Trusted Signing (now branded "Artifact Signing").

## Cutting a release

1. Land your changes on `main`, with `package.json` bumped and `CHANGELOG.md`
   updated (per `.claude/rules/commit-changelog.md`).
2. Tag the commit with a matching `v` + SemVer tag and push it:
   ```bash
   git tag v0.13.0
   git push origin v0.13.0
   ```
   The tag version must equal `package.json`'s `version`, or the workflow
   fails before building.
3. The workflow builds `hark-app` in release, signs the exe, verifies the
   signature (valid + timestamped) and publishes a GitHub release named
   `Hark <version>` with `Hark-<version>-windows-x64.exe` attached and
   auto-generated notes.

`workflow_dispatch` (Actions tab, "Release") is a manual fallback: it takes an
existing tag and does the same build-sign-publish.

## Required GitHub Actions secrets

Synced from Doppler into the repo's Actions secrets. The workflow reads these
exact names:

| Secret | What it is |
|---|---|
| `AZURE_CLIENT_ID` | Service principal (app registration) client ID |
| `AZURE_TENANT_ID` | Entra tenant ID |
| `AZURE_CLIENT_SECRET` | Service principal client secret |
| `AZURE_SIGNING_ENDPOINT` | Region endpoint, e.g. `https://eus.codesigning.azure.net/` (must match the account's region, or signing 403s) |
| `AZURE_SIGNING_ACCOUNT_NAME` | Trusted Signing account name |
| `AZURE_SIGNING_CERT_PROFILE_NAME` | Certificate profile name |

If the Doppler config uses different key names, either rename them in Doppler
or adjust the `secrets.*` references in the workflow.

## One-time Azure setup

- The service principal needs the **Trusted Signing Certificate Profile
  Signer** role, scoped to the certificate profile (or an account/resource
  group above it):
  ```bash
  az role assignment create \
    --assignee <sp-object-id> \
    --role "Trusted Signing Certificate Profile Signer" \
    --scope "/subscriptions/<sub>/resourceGroups/<rg>/providers/Microsoft.CodeSigning/codeSigningAccounts/<account>/certificateProfiles/<profile>"
  ```
- **Identity validation** on the Trusted Signing account must be complete
  before the profile can sign. This is a one-time review by Microsoft with a
  lead time of roughly 1 to 20 business days; do it well ahead of the first
  release. Public-trust validation is currently limited to organizations in
  the US, Canada, EU, and UK.

## Notes

- The signature is RFC3161-timestamped (`http://timestamp.acs.microsoft.com`),
  so it stays valid after the signing certificate rotates.
- Authenticode lives in the PE, not the filename, so renaming the signed exe
  to its release name does not break the signature; the workflow verifies
  after building and would fail the run if signing silently no-op'd.
