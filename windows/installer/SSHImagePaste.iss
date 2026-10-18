; Per-user installer for SSH Image Paste on Windows. Build with build-installer.ps1,
; which publishes the app and passes AppVersion, SourceDir, and OutputDir.

#ifndef AppVersion
  #error AppVersion must be defined, for example /DAppVersion=0.1.0
#endif
#ifndef SourceDir
  #error SourceDir must point at the self-contained publish folder
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#define AppName "SSH Image Paste"
#define AppExe "SSHImagePaste.exe"
#define RunValue "SSH Image Paste"

[Setup]
; Never change AppId: upgrades and uninstall are keyed on it.
AppId={{FAA5AFBA-AAD4-4AAC-BF30-8C4A89CBD5C3}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=SSH Image Paste
AppPublisherURL=https://github.com/SerenityTn/ssh-img-paste
AppSupportURL=https://github.com/SerenityTn/ssh-img-paste/issues
VersionInfoVersion={#AppVersion}
; Per-user install: no administrator rights, installs under %LOCALAPPDATA%\Programs.
PrivilegesRequired=lowest
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
; Fixed folder: upgrades clear it, so it must never be a folder the user chose.
DisableDirPage=yes
UsePreviousAppDir=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
; Windows 10 1809 ships the OpenSSH client the app relies on.
MinVersion=10.0.17763
SetupIconFile=..\SSHImagePaste.Windows\Assets\AppIcon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; Close a running copy during upgrade and uninstall through Restart Manager.
CloseApplications=yes
RestartApplications=no
OutputDir={#OutputDir}
OutputBaseFilename=SSHImagePaste-Setup-{#AppVersion}-x64
LicenseFile=..\..\LICENSE

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "startup"; Description: "Start {#AppName} when I sign in"; GroupDescription: "Options:"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs
; Remove the previous version's files that a newer build no longer ships.
[InstallDelete]
Type: filesandordirs; Name: "{app}\*"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; Same value the app's "Start when I sign in" checkbox manages.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "{#RunValue}"; ValueData: """{app}\{#AppExe}"" --background"; Tasks: startup; Flags: uninsdeletevalue
; Always remove the value on uninstall, even if it was enabled from the app.
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueName: "{#RunValue}"; Flags: uninsdeletevalue dontcreatekey noerror

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/IM {#AppExe} /F"; Flags: runhidden; RunOnceId: "StopApp"

; Destinations in %LOCALAPPDATA%\SSH Image Paste are kept on uninstall, like the macOS edition.

[UninstallDelete]
; Remove the app folder once its files are gone; settings live elsewhere and are kept.
Type: dirifempty; Name: "{app}"
