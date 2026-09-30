# macOS distribution

Hark supports Apple Silicon (ARM64) only on **macOS 14.2 or later**. This minimum
covers Core Audio process taps for meeting capture and SMAppService login items.
The app is a menu bar utility (`LSUIElement`), outside the App Sandbox, with the
hardened runtime and only the audio-input entitlement. Microphone and system
audio permission usage strings are included. Input Monitoring approval is needed for global shortcuts, and Accessibility
approval is needed for paste injection; both are controlled by the user
in System Settings, never granted by an entitlement.

Build and package locally on an Apple Silicon Mac:

```sh
MACOSX_DEPLOYMENT_TARGET=14.2 cargo build --release -p hark-app
bash scripts/package-macos.sh target/release/hark-app dist 0.60.4 arm64
```

Use the workspace's current version and `arm64`. Intel Mac packages are not supported. This produces an ad-hoc
signed `dist/Hark.app` and a DMG for local inspection; it is not a distributable
Developer ID build. Copy Hark.app from the disk image to a writable Applications
folder before running it. Signed bundles are necessary for login items and a
stable privacy-permission identity. The package gate rejects non-system dynamic
libraries; the on-device engine is linked statically.

`release.yml` builds/tests on an Apple Silicon runner and publishes versioned
`-macos-arm64.dmg` assets. It requires these repository secrets:

- `MACOS_CERTIFICATE_P12_BASE64`: exported Developer ID Application identity
- `MACOS_CERTIFICATE_PASSWORD`: export password
- `MACOS_SIGNING_IDENTITY`: exact Developer ID Application identity name
- `MACOS_APPLE_ID`: Apple developer account
- `MACOS_TEAM_ID`: that account's ten-character team ID
- `MACOS_APP_SPECIFIC_PASSWORD`: notarization app-specific password

Release packaging signs the bundle, notarizes and staples it, then signs,
notarizes and staples the DMG. Missing credentials fail the Mac release job;
Windows and Linux retain their existing release jobs.

The in-app updater selects only the Apple Silicon DMG. It mounts the image
read-only, verifies its full app signature against Apple's Developer ID chain,
the installed app's team ID and `com.boardpandas.hark`, and requires Gatekeeper
assessment. An unsigned/ad-hoc installed app cannot authorize an update. The
app is copied to a private temporary sibling directory and checked again before
a detached helper waits for exit, renames bundles and relaunches through Launch
Services. A failed rename or launch request restores the previous bundle.
Unwritable/translocated installations fail before quitting; move the app first.
A failed detached update leaves `install.log` and the staged/previous bundle in
`.hark-update-*` beside the app for recovery. Successful updates remove them.

Hardware acceptance on Apple Silicon remains required: first-run privacy
prompts, login/logout, permissions revoked in System Settings, cloud/on-device
dictation into another app, meeting system audio, and a signed update followed
by relaunch. CI checks compilation, tests, bundle metadata, signing structure,
architecture and dylib closure; it cannot grant a desktop user's privacy consent.
