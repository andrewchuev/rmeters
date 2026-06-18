#define MyAppName      "RMeters"
; Passed in from CI as /DMyAppVersion=x.y.z, falls back to Cargo.toml version for local builds
#ifndef MyAppVersion
  #define MyAppVersion "0.1.0"
#endif
#define MyAppPublisher "Andrew A. Chuev"
#define MyAppURL       "https://github.com/andrewchuev/rmeters"
#define MyAppExeName   "rmeters.exe"

[Setup]
AppId={{963F6A3A-7D73-4368-8ABE-BBB686F65957}
AppName={#MyAppName}
AppVersion={#MyAppVersion}
AppPublisher={#MyAppPublisher}
AppPublisherURL={#MyAppURL}
AppSupportURL={#MyAppURL}/issues
AppUpdatesURL={#MyAppURL}/releases
DefaultDirName={autopf}\{#MyAppName}
DisableProgramGroupPage=yes
; Installer does not require admin rights — installs per-user by default,
; but prompts for elevation if the user wants a system-wide install.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
OutputDir=dist
OutputBaseFilename=rmeters-setup
SetupIconFile=app_icon.ico
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
; Minimum supported OS: Windows 10
MinVersion=10.0

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "russian"; MessagesFile: "compiler:Languages\Russian.isl"

[Tasks]
Name: "startup"; Description: "{cm:AutoStartTask}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "target\release\{#MyAppExeName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#MyAppName}"; Filename: "{app}\{#MyAppExeName}"

[Registry]
; Add to Windows startup when the user selects the "startup" task
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; \
  ValueType: string; ValueName: "{#MyAppName}"; \
  ValueData: """{app}\{#MyAppExeName}"""; \
  Flags: uninsdeletevalue; Tasks: startup

[Run]
Filename: "{app}\{#MyAppExeName}"; \
  Description: "{cm:LaunchProgram,{#StringChange(MyAppName, '&', '&&')}}"; \
  Flags: nowait postinstall skipifsilent

[CustomMessages]
english.AutoStartTask=Launch {#MyAppName} automatically when Windows starts
russian.AutoStartTask=Запускать {#MyAppName} автоматически при старте Windows
