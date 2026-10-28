; Noita Save Selector installer (Inno Setup 6)
; Build: installer\build.ps1  (or: ISCC /DAppVersion=1.0.0 installer\NoitaSaveSelector.iss)

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
#ifndef AppGuid
  ; Only overridden for test installs next to a real one.
  #define AppGuid "6E1F3C2A-9B47-4C1D-8E55-0A7C2B9D4F11"
#endif
AppId={{{#AppGuid}}
AppName=Noita Save Selector
AppVersion={#AppVersion}
AppVerName=Noita Save Selector {#AppVersion}
AppPublisher=Rayyrawr
AppPublisherURL=https://github.com/Rayyrawr/NoitaSaveSelector
AppUpdatesURL=https://github.com/Rayyrawr/NoitaSaveSelector/releases
DefaultDirName={localappdata}\Programs\NoitaSaveSelector
DefaultGroupName=Noita Save Selector
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=NoitaSaveSelector-Setup-{#AppVersion}
SetupIconFile=app.ico
UninstallDisplayIcon={app}\NoitaSaveSelector.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
#ifndef AppMutexName
  #define AppMutexName "NoitaSaveSelectorMutex"
#endif
AppMutex={#AppMutexName}
CloseApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\app\target\release\NoitaSaveSelector.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Noita Save Selector"; Filename: "{app}\NoitaSaveSelector.exe"
Name: "{group}\Data folder"; Filename: "{code:GetDataDir}"
Name: "{group}\{cm:UninstallProgram,Noita Save Selector}"; Filename: "{uninstallexe}"
Name: "{autodesktop}\Noita Save Selector"; Filename: "{app}\NoitaSaveSelector.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\NoitaSaveSelector.exe"; Description: "{cm:LaunchProgram,Noita Save Selector}"; Flags: nowait postinstall skipifsilent

[UninstallDelete]
Type: files; Name: "{app}\data_dir.txt"
Type: files; Name: "{app}\*.old.exe"
Type: files; Name: "{app}\*.update.exe"
Type: dirifempty; Name: "{app}"

[Code]
var
  DataPage: TInputDirWizardPage;
  Detected: String;

function GetDataDir(Param: String): String;
begin
  Result := DataPage.Values[0];
end;

procedure InitializeWizard;
begin
  DataPage := CreateInputDirPage(wpSelectDir,
    'Data folder',
    'Where should profiles, snapshots and logs be stored?',
    'This is where the stored saves go (about 50-150 MB per profile, plus snapshots).' + #13#10 +
    'Updating or reinstalling never touches this folder.' + #13#10#13#10 +
    'Used the selector portably before? Pick that old folder here ' +
    '(the one containing state.json and profiles\) and everything is reused.',
    False, '');
  DataPage.Add('');
  // /DATADIR=... on the command line wins (scripted installs).
  DataPage.Values[0] := ExpandConstant('{param:DATADIR|' + GetPreviousData('DataDir', ExpandConstant('{localappdata}\NoitaSaveSelector')) + '}');
end;

procedure RegisterPreviousData(PreviousDataKey: Integer);
begin
  SetPreviousData(PreviousDataKey, 'DataDir', DataPage.Values[0]);
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + 'Data folder:' + NewLine + Space + DataPage.Values[0];
  if MemoTasksInfo <> '' then
    Result := Result + NewLine + NewLine + MemoTasksInfo;
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  Code: Integer;
  Lines: TArrayOfString;
  I: Integer;
begin
  if CurStep = ssPostInstall then
  begin
    ForceDirectories(DataPage.Values[0]);
    SaveStringToFile(ExpandConstant('{app}\data_dir.txt'), DataPage.Values[0], False);
    // Let the app look for Steam, Noita, the Entangled Worlds proxy and Noita Together.
    WizardForm.StatusLabel.Caption := 'Looking for Noita, Steam, Entangled Worlds and Noita Together ...';
    Exec(ExpandConstant('{app}\NoitaSaveSelector.exe'), '--detect', '', SW_HIDE, ewWaitUntilTerminated, Code);
    Detected := '';
    if LoadStringsFromFile(AddBackslash(DataPage.Values[0]) + 'detected.txt', Lines) then
      for I := 0 to GetArrayLength(Lines) - 1 do
        Detected := Detected + #13#10 + '  ' + Lines[I];
  end;
end;

procedure CurPageChanged(CurPageID: Integer);
begin
  if (CurPageID = wpFinished) and (Detected <> '') then
    WizardForm.FinishedLabel.Caption := WizardForm.FinishedLabel.Caption + #13#10#13#10 +
      'Programs found:' + Detected + #13#10#13#10 +
      'Anything missing can be entered in the app under Settings, or searched for again there.';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  // The app's own Uninstall button runs this silently after asking itself.
  if (CurUninstallStep = usPostUninstall) and not UninstallSilent then
    MsgBox('Your profiles and backups were NOT deleted.' + #13#10 +
      'They are still in the data folder (default: %LOCALAPPDATA%\NoitaSaveSelector).', mbInformation, MB_OK);
end;
