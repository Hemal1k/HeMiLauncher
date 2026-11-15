; HeMi Launcher installer (Inno Setup 6), built by release.ps1:  ISCC /DAppVersion=1.0.0 /DAppExe=<HeMiLauncher.exe> /DOutputDir=<folder>
; For the current user (no administrator) or for all users - the installer asks. /UPDATE (the launcher updates itself):
; quiet, the launcher starts again hidden in the notification area. Uninstalling removes the start with Windows, the
; place the other HeMi apps find the launcher by, and its notifications.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef AppExe
  #define AppExe "..\build\release\HeMiLauncher.exe"
#endif
#ifndef AppExeArm
  #define AppExeArm ""
#endif
#ifndef OutputDir
  #define OutputDir "..\build\dist"
#endif

[Setup]
AppId=HeMiLauncher
AppName=HeMi Launcher
AppVersion={#AppVersion}
AppVerName=HeMi Launcher {#AppVersion}
AppPublisher=Richard Hemala
AppPublisherURL=https://github.com/Hemal1k/HeMiLauncher
AppSupportURL=https://github.com/Hemal1k/HeMiLauncher/issues
AppUpdatesURL=https://github.com/Hemal1k/HeMiLauncher/releases
DefaultDirName={autopf}\HeMiLauncher
DisableDirPage=yes
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
; the first page asks: for me only (default, no administrator) or for all users (Program Files, asks for administrator)
PrivilegesRequiredOverridesAllowed=dialog commandline
ArchitecturesAllowed=x64compatible or arm64
ArchitecturesInstallIn64BitMode=x64compatible or arm64
MinVersion=10.0
OutputDir={#OutputDir}
OutputBaseFilename=HeMiLauncher-Setup
SetupIconFile=..\assets\HeMiLauncher.ico
UninstallDisplayIcon={app}\HeMiLauncher.exe
UninstallDisplayName=HeMi Launcher
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
LicenseFile=..\LICENSE

[Languages]
; English only
Name: "en"; MessagesFile: "compiler:Default.isl"

[Files]
; x64, and the ARM64 program on Windows on ARM when the installer has it
#if AppExeArm != ""
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion; Check: not IsArm64
Source: "{#AppExeArm}"; DestDir: "{app}"; DestName: "HeMiLauncher.exe"; Flags: ignoreversion; Check: IsArm64
#else
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Tasks]
Name: "autostart"; Description: "Start with Windows (hidden in the notification area, checks for updates)"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Icons]
Name: "{autoprograms}\HeMi Launcher"; Filename: "{app}\HeMiLauncher.exe"
Name: "{autodesktop}\HeMi Launcher"; Filename: "{app}\HeMiLauncher.exe"; Tasks: desktopicon

[Run]
; the start with Windows is the user's (also when installed for all users), so the launcher writes it itself
Filename: "{app}\HeMiLauncher.exe"; Parameters: "--autostart --tray"; Flags: nowait runasoriginaluser; Tasks: autostart; Check: not WizardSilent
Filename: "{app}\HeMiLauncher.exe"; Description: "{cm:LaunchProgram,HeMi Launcher}"; Flags: nowait postinstall skipifsilent runasoriginaluser
; after its own update (quiet): the launcher starts again in the notification area
Filename: "{app}\HeMiLauncher.exe"; Parameters: "--tray"; Flags: nowait runasoriginaluser; Check: WizardSilent

[UninstallRun]
Filename: "{sys}\taskkill.exe"; Parameters: "/F /IM HeMiLauncher.exe"; Flags: runhidden; RunOnceId: "StopLauncher"
; the task of the start at sign-in (the launcher makes it for the user)
Filename: "{sys}\schtasks.exe"; Parameters: "/Delete /TN ""HeMiLauncher ({username})"" /F"; Flags: runhidden; RunOnceId: "LauncherTask"

[Registry]
; what the launcher writes for the user: removed with it
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: none; ValueName: "HeMiLauncher"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\HeMi\HeMiLauncher"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\HeMi"; Flags: uninsdeletekeyifempty dontcreatekey
Root: HKCU; Subkey: "Software\Classes\AppUserModelId\HeMi.HeMiLauncher"; Flags: uninsdeletekey dontcreatekey

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\HeMi\HeMiLauncher"
Type: dirifempty; Name: "{localappdata}\HeMi"

[Code]
// Uninstalling: the settings of the app (%AppData%\HeMi\HeMiLauncher) are deleted or kept, as the user chooses (a quiet
// uninstall keeps them). %AppData%\HeMi goes too when no other HeMi app has anything there.
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Dir: String;
begin
  if CurUninstallStep <> usPostUninstall then Exit;
  Dir := ExpandConstant('{userappdata}\HeMi\HeMiLauncher');
  if not DirExists(Dir) then Exit;
  if UninstallSilent then Exit;
  if MsgBox('Delete the settings of HeMi Launcher too?' + #13#10#13#10 + 'Keep them to use them again when you install HeMi Launcher later.', mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
  begin
    DelTree(Dir, True, True, True);
    RemoveDir(ExpandConstant('{userappdata}\HeMi'));
  end;
end;
