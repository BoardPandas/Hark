; Hark per-user Windows installer (Inno Setup 6).
;
; Design (confirmed with the user 2026-07-17):
;   - Per-user install to %LOCALAPPDATA%\Programs\Hark, NO admin / NO UAC.
;     PrivilegesRequired=lowest makes {autopf} resolve there instead of
;     Program Files.
;   - Autostart is app-managed at runtime (hark-autostart writes the same HKCU
;     Run value). The installer seeds that value so launch-at-login works
;     immediately after a fresh install, and clears it on uninstall so it can
;     never point at a deleted exe. The app reconciles the value on every
;     launch and Save, so the two always agree.
;   - User data (%APPDATA%\hark: config.toml + history.db) is intentionally
;     left in place on uninstall.
;
; CI (release.yml) invokes:
;   iscc /DAppVersion=<x.y.z> /DSourceExe=<path to SIGNED hark-app.exe> installer\hark.iss
; producing installer\Output\Hark-<ver>-windows-x64-setup.exe, which CI then
; signs and verifies. The bundled exe is already signed before this runs
; (sign the app exe BEFORE iscc; sign the setup exe after).

#ifndef AppVersion
  #define AppVersion "0.0.0-dev"
#endif
#ifndef SourceExe
  ; Local default so `iscc installer\hark.iss` works after a local
  ; `cargo build --release -p hark-app`, without passing /D.
  #define SourceExe "..\target\release\hark-app.exe"
#endif

#define AppName "Hark"
#define AppPublisher "BoardPandas"
#define AppExeName "Hark.exe"
#define AppUrl "https://github.com/BoardPandas/Hark"
#define RunValueName "Hark"

[Setup]
; AppId keys upgrades and uninstall. NEVER change it.
AppId={{8F2A6C31-7B4E-4E2A-9D1C-3A5B6E8F0A21}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}/issues
AppUpdatesURL={#AppUrl}/releases
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Per-user, no elevation.
PrivilegesRequired=lowest
; The exe is x64; refuse 32-bit Windows where it cannot run.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
OutputDir=Output
OutputBaseFilename=Hark-{#AppVersion}-windows-x64-setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
UninstallDisplayIcon={app}\{#AppExeName}
UninstallDisplayName={#AppName}
; An autostarted Hark is likely running during an upgrade; let the Restart
; Manager close it so its exe can be replaced. It relaunches via the [Run]
; step (interactive) or at next login (autostart).
CloseApplications=yes
RestartApplications=no
SetupMutex=HarkSetupMutex

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "{#AppExeName}"; Flags: ignoreversion

[Icons]
Name: "{group}\{#AppName}"; Filename: "{app}\{#AppExeName}"
Name: "{group}\Uninstall {#AppName}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExeName}"; Tasks: desktopicon

[Registry]
; Seed launch-at-login (default on). Data mirrors hark-autostart's format
; exactly: "<exe>" --hidden, path quoted so a space in the dir cannot split
; the command at login. uninsdeletevalue removes it on uninstall.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
    ValueType: string; ValueName: "{#RunValueName}"; \
    ValueData: """{app}\{#AppExeName}"" --hidden"; \
    Flags: uninsdeletevalue

[Run]
; No --hidden here: a fresh install's first launch should show the window
; (onboarding, since no STT key is configured yet). skipifsilent keeps
; unattended installs headless.
Filename: "{app}\{#AppExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; \
    Flags: nowait postinstall skipifsilent
