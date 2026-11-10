#define AppName "Carambola"
#define AppVersion GetEnv("CARAMBOLA_VERSION")
#define AppExe GetEnv("CARAMBOLA_EXE")
#define AppIcon GetEnv("CARAMBOLA_ICON")
#define AppReadme GetEnv("CARAMBOLA_INSTALL_README")

#if AppVersion == ""
  #error CARAMBOLA_VERSION must be set by scripts/build-windows-installer.ps1
#endif
#if AppExe == ""
  #error CARAMBOLA_EXE must point to the built Windows release executable
#endif
#if AppIcon == ""
  #error CARAMBOLA_ICON must point to the Carambola icon
#endif
#if AppReadme == ""
  #error CARAMBOLA_INSTALL_README must point to the installation notes
#endif

[Setup]
AppId={{B6D6EAF0-79C2-4C0F-96D1-AB55823428E1}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=Carambola
AppPublisherURL=https://github.com/ahmetfsyn/carambola
AppSupportURL=https://github.com/ahmetfsyn/carambola/issues
AppReadmeFile={app}\README.txt
DefaultDirName={localappdata}\Programs\Carambola
DefaultGroupName=Carambola
DisableProgramGroupPage=yes
UsePreviousAppDir=yes
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupArchitecture=x64
Uninstallable=yes
CreateUninstallRegKey=yes
UninstallDisplayName=Carambola
UninstallDisplayIcon={app}\Carambola.exe
CloseApplications=no
RestartApplications=no
SetupIconFile="{#AppIcon}"
InfoAfterFile="{#AppReadme}"
OutputBaseFilename=Carambola-Setup-{#AppVersion}
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
VersionInfoCompany=Carambola
VersionInfoDescription=Carambola Windows x64 installer
VersionInfoOriginalFileName=Carambola-Setup-{#AppVersion}.exe
VersionInfoProductName=Carambola
VersionInfoProductVersion={#AppVersion}
VersionInfoProductTextVersion={#AppVersion}
VersionInfoVersion={#AppVersion}
VersionInfoTextVersion={#AppVersion}

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Files]
Source: "{#AppExe}"; DestDir: "{app}"; DestName: "Carambola.exe"; Flags: ignoreversion
Source: "{#AppReadme}"; DestDir: "{app}"; DestName: "README.txt"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Carambola"; Filename: "{app}\Carambola.exe"; WorkingDir: "{app}"; AppUserModelID: "io.carambola.desktop"
Name: "{autodesktop}\Carambola"; Filename: "{app}\Carambola.exe"; WorkingDir: "{app}"; Tasks: desktopicon

[InstallDelete]
Type: files; Name: "{userprograms}\Carambola Screenshot Notifications.lnk"
Type: files; Name: "{userprograms}\Carambola\Carambola.lnk"
Type: files; Name: "{userprograms}\Carambola\Uninstall Carambola.lnk"

[Code]
function InitializeSetup(): Boolean;
begin
  if CheckForMutexes('Local\Carambola.Native.9B0C7DC2-E7ED-4CF6-A9A4-3A5E7E871A45') then
  begin
    MsgBox('Carambola is running. Close it before installing or upgrading. Setup will not terminate it automatically.', mbInformation, MB_OK);
    Result := False;
  end
  else
    Result := True;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  SettingsPath: String;
  StartupCommand: String;
begin
  if CurStep = ssPostInstall then
  begin
    SettingsPath := ExpandConstant('{userappdata}\Carambola\settings.json');
    { Existing settings may contain an explicit OFF choice; let the app migrate and reconcile it. }
    if not FileExists(SettingsPath) then
    begin
      StartupCommand := '"' + ExpandConstant('{app}\Carambola.exe') + '"';
      if not RegWriteStringValue(
        HKCU,
        'Software\Microsoft\Windows\CurrentVersion\Run',
        'Carambola',
        StartupCommand) then
        MsgBox('Windows could not register Carambola to start when you sign in. Open Settings to retry.', mbError, MB_OK);
    end;
  end;
end;

function InitializeUninstall(): Boolean;
begin
  if CheckForMutexes('Local\Carambola.Native.9B0C7DC2-E7ED-4CF6-A9A4-3A5E7E871A45') then
  begin
    MsgBox('Carambola is running. Close it before uninstalling. The uninstaller will not terminate it automatically.', mbInformation, MB_OK);
    Result := False;
  end
  else
    Result := True;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usUninstall then
    if RegValueExists(
      HKCU,
      'Software\Microsoft\Windows\CurrentVersion\Run',
      'Carambola') then
      RegDeleteValue(
        HKCU,
        'Software\Microsoft\Windows\CurrentVersion\Run',
        'Carambola');
end;
