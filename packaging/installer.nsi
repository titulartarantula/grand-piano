; Grand Piano: Windows setup program (NSIS 3).
;
; Build it with packaging/build-installer.sh, which passes:
;   /DVERSION=x.y.z   version from Cargo.toml
;   /DSTAGE=dir       folder holding GrandPiano.exe, GrandPiano.ico, LICENSE,
;                     LICENSE-SAMPLES.txt and README.md
;   /DOUTFILE=path    where to write GrandPiano-Setup-x.y.z.exe
;
; Per-user install, no admin rights: %LOCALAPPDATA%\Programs\GrandPiano, a Start
; menu shortcut, an optional desktop shortcut, and an entry in "Installed apps"
; (HKCU). Settings (%APPDATA%\GrandPiano) and samples (%LOCALAPPDATA%\GrandPiano)
; are kept on upgrade and uninstall unless the user ticks "Remove settings and
; samples" in the uninstaller (or runs it with /PURGE).
;
; Silent use:  GrandPiano-Setup-x.y.z.exe /S [/DESKTOP]
;              uninstall.exe /S [/PURGE]

Unicode true
Target amd64-unicode
SetCompressor /SOLID lzma
RequestExecutionLevel user
ManifestDPIAware true

!ifndef VERSION
  !error "Pass /DVERSION=x.y.z"
!endif
!ifndef STAGE
  !error "Pass /DSTAGE=<folder with the files to install>"
!endif
!ifndef OUTFILE
  !define OUTFILE "GrandPiano-Setup-${VERSION}.exe"
!endif

!define APPNAME   "Grand Piano"
!define EXENAME   "GrandPiano.exe"
!define PUBLISHER "Jordan Holmes"
!define URL       "https://github.com/titulartarantula/grand-piano"
!define UNINSTKEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\GrandPiano"

!include "MUI2.nsh"
!include "FileFunc.nsh"
!include "LogicLib.nsh"

Name "${APPNAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\GrandPiano"
InstallDirRegKey HKCU "${UNINSTKEY}" "InstallLocation"
BrandingText "${APPNAME} ${VERSION}"

VIProductVersion "${VERSION}.0"
VIAddVersionKey "ProductName" "${APPNAME}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "FileDescription" "${APPNAME} setup"
VIAddVersionKey "CompanyName" "${PUBLISHER}"
VIAddVersionKey "LegalCopyright" "Copyright (c) 2026 ${PUBLISHER}. MIT License."

!define MUI_ICON   "${STAGE}\GrandPiano.ico"
!define MUI_UNICON "${STAGE}\GrandPiano.ico"
!define MUI_ABORTWARNING
!define MUI_COMPONENTSPAGE_NODESC

!define MUI_WELCOMEPAGE_TEXT "This will install ${APPNAME} ${VERSION} for your Windows account. No administrator rights are needed.$\r$\n$\r$\nIf ${APPNAME} is already installed, it will be upgraded. Your settings, MIDI mappings and downloaded piano samples are kept.$\r$\n$\r$\nThe piano samples (about 750 MB) are downloaded the first time you start the app."
!insertmacro MUI_PAGE_WELCOME
!define MUI_LICENSEPAGE_TEXT_TOP "The app is MIT-licensed. The piano samples it downloads are CC BY 3.0."
!define MUI_LICENSEPAGE_BUTTON "$(^NextBtn)"
!define MUI_LICENSEPAGE_TEXT_BOTTOM "Click Next to continue."
!insertmacro MUI_PAGE_LICENSE "${STAGE}\LICENSES.txt"
!insertmacro MUI_PAGE_COMPONENTS
!insertmacro MUI_PAGE_INSTFILES
!define MUI_FINISHPAGE_RUN "$INSTDIR\${EXENAME}"
!define MUI_FINISHPAGE_RUN_TEXT "Start ${APPNAME}"
!insertmacro MUI_PAGE_FINISH

!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_COMPONENTS
!insertmacro MUI_UNPAGE_INSTFILES

!insertmacro MUI_LANGUAGE "English"

; The exe can't be replaced or removed while it runs, and closing it behind the
; player's back would lose a performance. A running exe can't be opened for
; writing, so try that on our own copy only (other users' copies don't matter).
; Silent mode: give up with exit code 2 instead of asking.
!macro CHECK_NOT_RUNNING
  ${If} ${FileExists} "$INSTDIR\${EXENAME}"
    ${Do}
      ClearErrors
      FileOpen $0 "$INSTDIR\${EXENAME}" a
      ${IfNot} ${Errors}
        FileClose $0
        ${Break}
      ${EndIf}
      ${If} ${Cmd} `MessageBox MB_RETRYCANCEL|MB_ICONEXCLAMATION "${APPNAME} is running. Close it, then click Retry." /SD IDCANCEL IDCANCEL`
        SetErrorLevel 2
        Quit
      ${EndIf}
    ${Loop}
  ${EndIf}
!macroend

Section "${APPNAME}" SecApp
  SectionIn RO
  !insertmacro CHECK_NOT_RUNNING
  SetOutPath "$INSTDIR"
  File "${STAGE}\${EXENAME}"
  File "${STAGE}\GrandPiano.ico"
  File "${STAGE}\LICENSE"
  File "${STAGE}\LICENSE-SAMPLES.txt"
  File "${STAGE}\README.md"
  ; Left behind by the old script installer; uninstall.exe replaces it.
  Delete "$INSTDIR\uninstall.ps1"
  WriteUninstaller "$INSTDIR\uninstall.exe"

  SetShellVarContext current
  CreateShortcut "$SMPROGRAMS\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\GrandPiano.ico" 0 SW_SHOWNORMAL "" "Sampled grand piano and synths for your MIDI keyboard"

  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayName"          "${APPNAME}"
  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayVersion"       "${VERSION}"
  WriteRegStr   HKCU "${UNINSTKEY}" "Publisher"            "${PUBLISHER}"
  WriteRegStr   HKCU "${UNINSTKEY}" "DisplayIcon"          "$INSTDIR\GrandPiano.ico"
  WriteRegStr   HKCU "${UNINSTKEY}" "InstallLocation"      "$INSTDIR"
  WriteRegStr   HKCU "${UNINSTKEY}" "UninstallString"      '"$INSTDIR\uninstall.exe"'
  WriteRegStr   HKCU "${UNINSTKEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
  WriteRegStr   HKCU "${UNINSTKEY}" "URLInfoAbout"         "${URL}"
  WriteRegStr   HKCU "${UNINSTKEY}" "HelpLink"             "${URL}#readme"
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoModify" 1
  WriteRegDWORD HKCU "${UNINSTKEY}" "NoRepair" 1
  ${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
  WriteRegDWORD HKCU "${UNINSTKEY}" "EstimatedSize" "$0"
SectionEnd

Section /o "Desktop shortcut" SecDesktop
  SetShellVarContext current
  CreateShortcut "$DESKTOP\${APPNAME}.lnk" "$INSTDIR\${EXENAME}" "" "$INSTDIR\GrandPiano.ico" 0 SW_SHOWNORMAL "" "Sampled grand piano and synths for your MIDI keyboard"
SectionEnd

Function .onInit
  ; /DESKTOP adds the desktop shortcut in a silent install.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/DESKTOP" $R1
  ${IfNot} ${Errors}
    SectionSetFlags ${SecDesktop} ${SF_SELECTED}
  ${EndIf}
  ; On upgrade, keep the desktop shortcut if there is one.
  SetShellVarContext current
  ${If} ${FileExists} "$DESKTOP\${APPNAME}.lnk"
    SectionSetFlags ${SecDesktop} ${SF_SELECTED}
  ${EndIf}
FunctionEnd

; ---------------------------------------------------------------- uninstaller

Section "un.${APPNAME}" UnSecApp
  SectionIn RO
  !insertmacro CHECK_NOT_RUNNING
  SetShellVarContext current
  Delete "$SMPROGRAMS\${APPNAME}.lnk"
  Delete "$DESKTOP\${APPNAME}.lnk"
  ; Remove only what we installed, then the folder if it's empty.
  Delete "$INSTDIR\${EXENAME}"
  Delete "$INSTDIR\GrandPiano.ico"
  Delete "$INSTDIR\LICENSE"
  Delete "$INSTDIR\LICENSE-SAMPLES.txt"
  Delete "$INSTDIR\README.md"
  Delete "$INSTDIR\uninstall.ps1"
  Delete "$INSTDIR\uninstall.exe"
  RMDir "$INSTDIR"
  DeleteRegKey HKCU "${UNINSTKEY}"
SectionEnd

Section /o "un.Remove settings and samples" UnSecData
  RMDir /r "$APPDATA\GrandPiano"
  RMDir /r "$LOCALAPPDATA\GrandPiano"
SectionEnd

Function un.onInit
  ; /PURGE removes settings and samples in a silent uninstall.
  ${GetParameters} $R0
  ClearErrors
  ${GetOptions} $R0 "/PURGE" $R1
  ${IfNot} ${Errors}
    SectionSetFlags ${UnSecData} ${SF_SELECTED}
  ${EndIf}
FunctionEnd
