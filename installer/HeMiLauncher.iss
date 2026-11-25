; HeMi Launcher installer (Inno Setup 6), built by release.ps1:  ISCC /DAppVersion=1.0.0 /DAppExe=<HeMiLauncher.exe> /DOutputDir=<folder>
; For the current user (no administrator) or for all users - the installer asks. /UPDATE (the launcher updates itself):
; quiet, the launcher starts again hidden in the notification area (with /OPEN=1 with its window). Uninstalling removes the start with Windows, the
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
DefaultDirName={autopf}\HeMi\HeMiLauncher
UsePreviousAppDir=no
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
; the look of Windows 11, light or dark as Windows, and as big as the first question of the installer (it was
; smaller and old-looking next to it, 2026-11-19)
WizardStyle=modern dynamic windows11
WizardSizePercent=120
CloseApplications=force
LicenseFile=..\LICENSE
; the language of Windows (Czech or English) without asking
ShowLanguageDialog=auto

[Languages]
; English, Czech on Czech Windows
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "cs"; MessagesFile: "compiler:Languages\Czech.isl"

[CustomMessages]
en.AutoStart=Start with Windows (hidden in the notification area, checks for updates)
cs.AutoStart=Spouštět s Windows (skrytě v oznamovací oblasti, kontroluje aktualizace)
en.UpdatesGroup=Updates:
cs.UpdatesGroup=Aktualizace:
en.Updates=Look for new versions automatically
cs.Updates=Automaticky kontrolovat nové verze
en.UpdatesInstall=Install them automatically (quietly, in the background)
cs.UpdatesInstall=Automaticky je instalovat (potichu na pozadí)
en.DeleteSettings=Delete the settings of HeMi Launcher too?
cs.DeleteSettings=Smazat i nastavení HeMi Launcheru?
en.KeepSettings=Keep them to use them again when you install HeMi Launcher later.
cs.KeepSettings=Ponech je, pokud je budeš chtít použít při pozdější instalaci HeMi Launcheru.

[Files]
; x64, and the ARM64 program on Windows on ARM when the installer has it
#if AppExeArm != ""
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion; Check: not IsArm64
Source: "{#AppExeArm}"; DestDir: "{app}"; DestName: "HeMiLauncher.exe"; Flags: ignoreversion; Check: IsArm64
#else
Source: "{#AppExe}"; DestDir: "{app}"; Flags: ignoreversion
#endif

[Tasks]
Name: "autostart"; Description: "{cm:AutoStart}"
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked
; the updates (written for the launcher to take at its start – see SaveUpdateChoice): two separate boxes (as parent and
; child, unticking the second unticked the first too, 2026-11-19); installing them needs looking for them
Name: "updates"; Description: "{cm:Updates}"; GroupDescription: "{cm:UpdatesGroup}"
Name: "updatesinstall"; Description: "{cm:UpdatesInstall}"; GroupDescription: "{cm:UpdatesGroup}"; Flags: unchecked

[Icons]
Name: "{autoprograms}\HeMi Launcher"; Filename: "{app}\HeMiLauncher.exe"
Name: "{autodesktop}\HeMi Launcher"; Filename: "{app}\HeMiLauncher.exe"; Tasks: desktopicon

[Run]
; the File Explorer items and the start with Windows point to this installation (also after a move from the old folder)
Filename: "{app}\HeMiLauncher.exe"; Parameters: "--repair"; Flags: runasoriginaluser waituntilterminated runhidden
; the start with Windows is the user's (also when installed for all users), so the launcher writes it itself
Filename: "{app}\HeMiLauncher.exe"; Parameters: "--set-autostart"; Flags: runasoriginaluser waituntilterminated runhidden; Tasks: autostart; Check: not WizardSilent
Filename: "{app}\HeMiLauncher.exe"; Description: "{cm:LaunchProgram,HeMi Launcher}"; Flags: nowait postinstall skipifsilent runasoriginaluser
; after its own update (quiet): the launcher starts again in the notification area – or with its window when the update
; was clicked there (/OPEN=1)
Filename: "{app}\HeMiLauncher.exe"; Parameters: "--tray"; Flags: nowait runasoriginaluser; Check: WizardSilent and not OpenAfter
Filename: "{app}\HeMiLauncher.exe"; Flags: nowait runasoriginaluser; Check: WizardSilent and OpenAfter

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
; the link a click on its notification opens (it brings up the window)
Root: HKCU; Subkey: "Software\Classes\hemilauncher"; Flags: uninsdeletekey dontcreatekey
; what Windows itself keeps about the app (its name in menus, "Open with", its notifications, Task Manager → Startup apps)
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run"; ValueType: none; ValueName: "HeMiLauncher"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"; ValueType: none; ValueName: "{app}\HeMiLauncher.exe.FriendlyAppName"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Classes\Local Settings\Software\Microsoft\Windows\Shell\MuiCache"; ValueType: none; ValueName: "{app}\HeMiLauncher.exe.ApplicationCompany"; Flags: uninsdeletevalue dontcreatekey
Root: HKCU; Subkey: "Software\Classes\Applications\HeMiLauncher.exe"; Flags: uninsdeletekey dontcreatekey
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Notifications\Settings\HeMi.HeMiLauncher"; Flags: uninsdeletekey dontcreatekey

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\HeMi\HeMiLauncher"
Type: dirifempty; Name: "{localappdata}\HeMi"
; what the launcher leaves in %TEMP% (the licences it showed, the skins of HeMi Dark while unpacking)
Type: files; Name: "{%TEMP}\HeMiLauncher-ThirdPartyNotices.txt"
Type: filesandordirs; Name: "{%TEMP}\HeMiLauncher-HeMiDark"

[Code]
// Since 2026-11 the HeMi apps go into one folder HeMi, as their settings do (%AppData%\HeMi\<app>): {autopf}\HeMi\HeMiLauncher.
// An installation in the old folder {autopf}\HeMiLauncher moves: the app running from there ends, the new version goes into the
// new folder ("--repair" points the File Explorer items and the start with Windows to it) and the old folder is deleted.
var
  OldFolder: String;

// /OPEN=1: the launcher updated itself from its window – it opens the window again after the update
function OpenAfter: Boolean;
begin
  Result := ExpandConstant('{param:OPEN|0}') = '1';
end;

function FindOldFolder: String;
begin
  if RegQueryStringValue(HKA, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiLauncher_is1', 'InstallLocation', Result) then
  begin
    Result := RemoveBackslashUnlessRoot(Result);
    if (CompareText(Result, ExpandConstant('{app}')) = 0) or (CompareText(ExtractFileName(Result), 'HeMiLauncher') <> 0)
      or not FileExists(Result + '\HeMiLauncher.exe') then
      Result := '';
  end
  else
    Result := '';
end;

// the folder goes into the command as a PowerShell string in apostrophes (an apostrophe in it doubled, as PowerShell
// wants) and is compared as text (StartsWith) - not with -like, which takes [ ] in a path for wildcards
procedure StopOld;
var
  Code: Integer;
  Folder: String;
begin
  OldFolder := FindOldFolder;
  if OldFolder = '' then Exit;
  Folder := OldFolder + '\';
  StringChangeEx(Folder, '''', '''''', True);
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -Command "$d = ''' + Folder + '''; Get-Process | Where-Object { $_.Path -and $_.Path.StartsWith($d, [System.StringComparison]::OrdinalIgnoreCase) } | Stop-Process -Force"',
    '', SW_HIDE, ewWaitUntilTerminated, Code);
end;

procedure DeleteOld;
begin
  if OldFolder <> '' then DelTree(OldFolder, True, True, True);
end;

var
  // HeMi Launcher was installed before (an update, a repair or a move from the old folder)
  Upgrade: Boolean;

function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  Result := '';
  Upgrade := RegKeyExists(HKLM, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiLauncher_is1')
    or RegKeyExists(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Uninstall\HeMiLauncher_is1');
  StopOld;
end;

{ the choice of the updates for the launcher to take at its start (HKCU\Software\HeMi\HeMiLauncher) – written before the
  launcher is started at the end. A quiet update keeps what was set in the app; a new installation and one by hand write it. }
procedure SaveUpdateChoice;
var
  Check, Install: String;
begin
  if WizardSilent and Upgrade then Exit;
  Check := '0'; Install := '0';
  if WizardIsTaskSelected('updates') then Check := '1';
  { installing needs looking: only with "Look for new versions" }
  if WizardIsTaskSelected('updates') and WizardIsTaskSelected('updatesinstall') then Install := '1';
  RegWriteStringValue(HKCU, 'Software\HeMi\HeMiLauncher', 'SetupAutoCheck', Check);
  RegWriteStringValue(HKCU, 'Software\HeMi\HeMiLauncher', 'SetupAutoInstall', Install);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if CurStep = ssInstall then SaveUpdateChoice;
  if CurStep = ssPostInstall then DeleteOld;
end;

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
  if MsgBox(CustomMessage('DeleteSettings') + #13#10#13#10 + CustomMessage('KeepSettings'), mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
  begin
    DelTree(Dir, True, True, True);
    RemoveDir(ExpandConstant('{userappdata}\HeMi'));
  end;
end;
