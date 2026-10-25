#define AppVersion "1.0.2"
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
OutputBaseFilename=VietKaraStudio-{#AppVersion}-Setup-x64
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
[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"
[Files]
Source: "{#SourceDir}\dependencies\MicrosoftEdgeWebview2Setup.exe"; Flags: dontcopy noencryption
Source: "{#SourceDir}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs; Excludes: "portable.flag,dependencies\*,*\__pycache__\*,*.pyc,*.pyo"
[Icons]
Name: "{group}\VietKara Studio"; Filename: "{app}\VietKaraStudio.exe"; AppUserModelID: "VietKaraStudio.Desktop"
Name: "{autodesktop}\VietKara Studio"; Filename: "{app}\VietKaraStudio.exe"; Tasks: desktopicon; AppUserModelID: "VietKaraStudio.Desktop"
[Run]
Filename: "{app}\VietKaraStudio.exe"; Description: "Open VietKara Studio"; Flags: nowait postinstall skipifsilent

[Code]
function HasWebView2In(RootKey: Integer): Boolean;
var
  Version: String;
begin
  Result := RegQueryStringValue(RootKey,
    'Software\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}', 'pv', Version)
    and (Version <> '') and (Version <> '0.0.0.0');
end;

function HasWebView2: Boolean;
begin
  Result := HasWebView2In(HKLM32) or HasWebView2In(HKCU32) or HasWebView2In(HKCU64);
end;

function PrepareToInstall(var NeedsRestart: Boolean): String;
var
  ExitCode: Integer;
begin
  Result := '';
  if HasWebView2 then Exit;
  Log('WebView2 Runtime is missing. Installing the bundled Microsoft Evergreen bootstrapper.');
  ExtractTemporaryFile('MicrosoftEdgeWebview2Setup.exe');
  WizardForm.StatusLabel.Caption := 'Installing Microsoft Edge WebView2 Runtime (Internet required)...';
  if not Exec(ExpandConstant('{tmp}\MicrosoftEdgeWebview2Setup.exe'), '/silent /install', '',
    SW_HIDE, ewWaitUntilTerminated, ExitCode) then begin
    Result := 'Could not start the Microsoft WebView2 installer. Check your Internet connection and retry.';
    Exit;
  end;
  Log('Microsoft WebView2 installer exit code: ' + IntToStr(ExitCode));
  if not HasWebView2 then
    Result := 'Microsoft Edge WebView2 Runtime could not be installed (code ' + IntToStr(ExitCode) + ').' + #13#10 +
      'Connect to the Internet and retry, or install WebView2 Evergreen Runtime from Microsoft before running Setup again.' + #13#10 +
      'https://developer.microsoft.com/microsoft-edge/webview2/';
end;
