# Releasing Hark

Hark ships as a signed Windows **installer** (`Hark-<version>-windows-x64-setup.exe`,
built from [`installer/hark.iss`](../installer/hark.iss) with Inno Setup) plus the
signed portable `.exe`, both built and published by
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
3. The workflow builds `hark-app` in release and signs the exe, then packages
   it into a per-user installer (Inno Setup, installed on the runner via
   `choco install innosetup`) and signs the installer too. Both signatures are
   verified (valid + timestamped). It publishes a GitHub release named
   `Hark <version>` with the installer `Hark-<version>-windows-x64-setup.exe`
   (headline) and the portable `Hark-<version>-windows-x64.exe` attached, plus
   auto-generated notes.

   The installer is per user (no admin), installs to `%LOCALAPPDATA%\Programs\Hark`,
   and seeds the launch-at-login registry entry the app then manages. See
   [`installer/hark.iss`](../installer/hark.iss); its `AppId` GUID is permanent
   (it keys upgrades and uninstall) and must never change.

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
| `AZURE_TRUSTED_SIGNING_ENDPOINT` | Region endpoint, e.g. `https://eus.codesigning.azure.net/` (must match the account's region, or signing 403s) |
| `AZURE_TRUSTED_SIGNING_ACCOUNT_NAME` | Trusted Signing account name |
| `AZURE_TRUSTED_SIGNING_CERT_PROFILE_NAME` | Certificate profile name |

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
