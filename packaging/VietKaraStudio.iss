#define AppVersion "1.0.0"
#ifndef SourceDir
  #error SourceDir must be supplied
#endif
[Setup]
AppId={{66458DB3-B288-4408-90D6-93142DF5AD36}
AppName=VietKara Studio
AppVersion={#AppVersion}
AppPublisher=VietKara Studio
DefaultDirName={localappdata}\Programs\VietKara Studio
DefaultGroupName=VietKara Studio
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\releases
OutputBaseFilename=VietKaraStudio-1.0.0-Setup-x64
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
VersionInfoVersion=1.0.0.0
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"
[Files]
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "portable.flag,*\__pycache__\*,*.pyc,*.pyo"
[Icons]
Name: "{group}\VietKara Studio"; Filename: "{app}\VietKaraStudio.exe"
Name: "{autodesktop}\VietKara Studio"; Filename: "{app}\VietKaraStudio.exe"; Tasks: desktopicon
[Run]
Filename: "{app}\VietKaraStudio.exe"; Description: "Open VietKara Studio"; Flags: nowait postinstall skipifsilent
