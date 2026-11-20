; Inno Setup script for Rusty POE Trade (Windows 11 x64).
; Build: iscc /DAppVersion=0.1.0 /DSourceExe=..\..\target\release\rusty-poe-trade.exe packaging\windows\installer.iss
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\..\target\release\rusty-poe-trade.exe"
#endif

[Setup]
AppId={{6C3A1E2B-6F7E-4C2B-9B7A-52A1D0F0B1E7}
AppName=Rusty POE Trade
AppVersion={#AppVersion}
AppPublisher=Rusty POE Trade contributors
AppPublisherURL=https://github.com/tylort69/rusty-poe-trade
DefaultDirName={localappdata}\Programs\Rusty POE Trade
DefaultGroupName=Rusty POE Trade
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0.17763
OutputDir=..\..\dist
OutputBaseFilename=RustyPoeTrade-Setup-{#AppVersion}
SetupIconFile=..\..\assets\icon.ico
UninstallDisplayIcon={app}\rusty-poe-trade.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
LicenseFile=..\..\LICENSE

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
Name: "autostart"; Description: "Start Rusty POE Trade when I sign in"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; DestName: "rusty-poe-trade.exe"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\..\LICENSE-UPSTREAM"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Rusty POE Trade"; Filename: "{app}\rusty-poe-trade.exe"
Name: "{autodesktop}\Rusty POE Trade"; Filename: "{app}\rusty-poe-trade.exe"; Tasks: desktopicon
Name: "{userstartup}\Rusty POE Trade"; Filename: "{app}\rusty-poe-trade.exe"; Tasks: autostart

[Run]
Filename: "{app}\rusty-poe-trade.exe"; Description: "{cm:LaunchProgram,Rusty POE Trade}"; Flags: nowait postinstall skipifsilent
