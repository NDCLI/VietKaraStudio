#ifndef SourceDir
  #error SourceDir must be supplied
#endif
#ifndef LauncherPath
  #error LauncherPath must be supplied
#endif
#ifndef UpdaterPath
  #error UpdaterPath must be supplied
#endif
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef InstallerOutputDir
  #define InstallerOutputDir "..\releases"
#endif

[Setup]
AppId={{A8298989-C825-4DDC-A1D6-FE8DAE3CFF97}
AppName=VietKara Studio Rust Preview
AppVersion={#AppVersion}
AppPublisher=VietKara Studio
DefaultDirName={localappdata}\Programs\VietKaraStudio-Rust
DefaultGroupName=VietKara Studio Rust Preview
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir={#InstallerOutputDir}
OutputBaseFilename=VietKaraStudio-Rust-{#AppVersion}-Setup-x64
SetupIconFile=VietKaraStudio.ico
UninstallDisplayIcon={app}\VietKaraStudio.exe
Compression=zip/3
SolidCompression=no
DiskSpanning=no
WizardStyle=modern
DisableProgramGroupPage=yes
CloseApplications=yes
RestartApplications=no
SetupLogging=yes
VersionInfoVersion={#AppVersion}.0
Uninstallable=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"

[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "data\*,VietKaraStudio.cmd,*\__pycache__\*,*.pyc,*.pyo"
Source: "{#LauncherPath}"; DestDir: "{app}"; DestName: "VietKaraStudio.exe"; Flags: ignoreversion
Source: "{#UpdaterPath}"; DestDir: "{app}"; DestName: "VietKaraUpdater.exe"; Flags: ignoreversion

[Icons]
Name: "{group}\VietKara Studio Rust Preview"; Filename: "{app}\VietKaraStudio.exe"; AppUserModelID: "VietKaraStudio.RustPreview"
Name: "{autodesktop}\VietKara Studio Rust Preview"; Filename: "{app}\VietKaraStudio.exe"; Tasks: desktopicon; AppUserModelID: "VietKaraStudio.RustPreview"

[Run]
Filename: "{app}\VietKaraStudio.exe"; Description: "Open VietKara Studio"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\installed.flag"

[Code]
procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssPostInstall then
    SaveStringToFile(ExpandConstant('{app}\installed.flag'), 'VietKara Studio Rust Setup', False);
end;
