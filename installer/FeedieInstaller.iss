#define AppVersion GetEnv("FEEDIE_VERSION")
#if AppVersion == ""
  #define AppVersion "0.0.0-dev"
#endif

[Setup]
AppId={{8DAC3CFD-3D3A-4D68-8DA0-AF11B6B11E52}}
AppName=Feedie
AppVersion={#AppVersion}
AppVerName=Feedie {#AppVersion}
AppPublisher=Feedie
AppPublisherURL=https://github.com/kpauly/feedie
AppSupportURL=https://github.com/kpauly/feedie
AppUpdatesURL=https://github.com/kpauly/feedie/releases
DefaultDirName={autopf}\Feedie
DefaultGroupName=Feedie
OutputDir={#SourcePath}\..\dist
OutputBaseFilename=FeedieSetup-{#AppVersion}
Compression=lzma
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
DisableDirPage=yes
DisableProgramGroupPage=yes
SetupLogging=yes
LicenseFile=..\LICENSE
UninstallDisplayIcon={app}\Feedie.exe
SetupIconFile=..\assets\Feedie.ico

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"
Name: "dutch"; MessagesFile: "compiler:Languages\Dutch.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "..\target\release\Feedie.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\FeedieUpdater.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\models\*"; DestDir: "{app}\models"; Flags: ignoreversion recursesubdirs createallsubdirs
Source: "..\manifest.json"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\Feedie"; Filename: "{app}\Feedie.exe"
Name: "{autodesktop}\Feedie"; Filename: "{app}\Feedie.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\Feedie.exe"; Description: "{cm:LaunchProgram,Feedie}"; Flags: nowait postinstall skipifsilent
