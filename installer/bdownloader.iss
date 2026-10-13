; Inno Setup script for BDownloader. Build with scripts\build-installer.ps1,
; which passes the version and the folder with the release binaries.
;
; Installs for the current user only (no admin prompt), into
; %LOCALAPPDATA%\Programs\BDownloader.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
#endif

; Extension IDs on the Chrome Web Store and Edge Add-ons. Once set, the
; browser offers to add the extension the next time it starts (like IDM).
; Browsers only allow this for extensions published on their store.
#define ChromeStoreId ""
#define EdgeStoreId ""

[Setup]
; Never change AppId: Windows uses it to find the existing installation on updates.
AppId={{6A1E3C52-8F0B-4D7E-9B2A-5C4F1D3E7A90}
AppName=BDownloader
AppVersion={#AppVersion}
AppVerName=BDownloader {#AppVersion}
AppPublisher=Ahmad Anjum
AppPublisherURL=https://github.com/ahmedanjum30/BDownloader
AppSupportURL=https://github.com/ahmedanjum30/BDownloader/issues
AppUpdatesURL=https://github.com/ahmedanjum30/BDownloader/releases
DefaultDirName={autopf}\BDownloader
DefaultGroupName=BDownloader
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
SetupIconFile=..\crates\app\assets\bdownloader.ico
UninstallDisplayIcon={app}\BDownloader.exe
UninstallDisplayName=BDownloader
WizardStyle=modern
WizardSizePercent=100
Compression=lzma2/max
SolidCompression=yes
OutputDir=..\dist
OutputBaseFilename=BDownloader-Setup-{#AppVersion}
; Closes a running BDownloader before replacing its files.
CloseApplications=force
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#BinDir}\BDownloader.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\bdl-host.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\extension\*"; DestDir: "{app}\extension"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "..\PRIVACY.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\BDownloader"; Filename: "{app}\BDownloader.exe"
Name: "{autodesktop}\BDownloader"; Filename: "{app}\BDownloader.exe"; Tasks: desktopicon

[Registry]
#if ChromeStoreId != ""
Root: HKCU; Subkey: "Software\Google\Chrome\Extensions\{#ChromeStoreId}"; ValueType: string; ValueName: "update_url"; ValueData: "https://clients2.google.com/service/update2/crx"; Flags: uninsdeletekey
#endif
#if EdgeStoreId != ""
Root: HKCU; Subkey: "Software\Microsoft\Edge\Extensions\{#EdgeStoreId}"; ValueType: string; ValueName: "update_url"; ValueData: "https://edge.microsoft.com/extensionwebstorebase/v1/crx"; Flags: uninsdeletekey
#endif

[Run]
; Lets the browser extension reach the app, even before the app is first opened.
Filename: "{app}\BDownloader.exe"; Parameters: "--register"; Flags: runhidden
Filename: "{app}\BDownloader.exe"; Description: "{cm:LaunchProgram,BDownloader}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; The app may be running hidden in the tray; stop it so its files can be removed.
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM BDownloader.exe"; Flags: runhidden; RunOnceId: "StopApp"
Filename: "{app}\BDownloader.exe"; Parameters: "--unregister"; Flags: runhidden; RunOnceId: "Unregister"
