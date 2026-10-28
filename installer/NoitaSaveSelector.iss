; Noita Save Selector – Installer (Inno Setup 6)
; Build: installer\build.ps1  (or: ISCC /DAppVersion=1.0.0 installer\NoitaSaveSelector.iss)

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6E1F3C2A-9B47-4C1D-8E55-0A7C2B9D4F11}
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
AppMutex=NoitaSaveSelectorMutex
CloseApplications=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible

[Languages]
Name: "de"; MessagesFile: "compiler:Languages\German.isl"
Name: "en"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "..\app\target\release\NoitaSaveSelector.exe"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Noita Save Selector"; Filename: "{app}\NoitaSaveSelector.exe"
Name: "{group}\Daten-Ordner"; Filename: "{code:GetDataDir}"
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
    'Daten-Ordner',
    'Wo sollen Profile, Snapshots und Logs liegen?',
    'Hier landen die gespeicherten Spielstände (pro Profil ca. 50-150 MB plus Snapshots).' + #13#10 +
    'Bei einem Update oder einer Neuinstallation bleibt dieser Ordner unangetastet.' + #13#10#13#10 +
    'Du hast den Selector schon portabel benutzt? Dann hier den alten Ordner wählen ' +
    '(der mit state.json und profiles\) - alles wird weiterverwendet.',
    False, '');
  DataPage.Add('');
  DataPage.Values[0] := GetPreviousData('DataDir', ExpandConstant('{localappdata}\NoitaSaveSelector'));
end;

procedure RegisterPreviousData(PreviousDataKey: Integer);
begin
  SetPreviousData(PreviousDataKey, 'DataDir', DataPage.Values[0]);
end;

function UpdateReadyMemo(Space, NewLine, MemoUserInfoInfo, MemoDirInfo, MemoTypeInfo, MemoComponentsInfo, MemoGroupInfo, MemoTasksInfo: String): String;
begin
  Result := MemoDirInfo + NewLine + NewLine + 'Daten-Ordner:' + NewLine + Space + DataPage.Values[0];
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
    WizardForm.StatusLabel.Caption := 'Suche Noita, Steam, Entangled Worlds und Noita Together ...';
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
      'Gefundene Programme:' + Detected + #13#10#13#10 +
      'Fehlendes kannst du in der App unter Einstellungen eintragen oder dort neu suchen lassen.';
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  if CurUninstallStep = usPostUninstall then
    MsgBox('Deine Profile und Backups wurden NICHT gelöscht.' + #13#10 +
      'Sie liegen weiterhin im Daten-Ordner (Standard: %LOCALAPPDATA%\NoitaSaveSelector).', mbInformation, MB_OK);
end;
