Unicode true
SetCompressor /SOLID lzma
RequestExecutionLevel user

!include "MUI2.nsh"
!include "LogicLib.nsh"
!include "FileFunc.nsh"

!ifndef VERSION
	!define VERSION "0.0.0"
!endif
!ifndef VERSION_QUAD
	!define VERSION_QUAD "0.0.0.0"
!endif
!ifndef BINARY
	!define BINARY "..\..\target\release\oxidcord.exe"
!endif
!ifndef OUTFILE
	!define OUTFILE "oxidcord-setup.exe"
!endif

!define APP_NAME "Oxidcord"
!define APP_ID "dev.oxidcord.Oxidcord"
!define APP_EXE "oxidcord.exe"
!define UNINSTALL_KEY "Software\Microsoft\Windows\CurrentVersion\Uninstall\${APP_NAME}"
!define SHORTCUT "$SMPROGRAMS\${APP_NAME}.lnk"

!if /FileExists "${__FILEDIR__}\plugins\x86-unicode\WinShell.dll"
	!addplugindir /x86-unicode "${__FILEDIR__}\plugins\x86-unicode"
	!define HAVE_WINSHELL
!endif

!insertmacro GetParameters
!insertmacro GetOptions
!insertmacro GetSize

Name "${APP_NAME}"
OutFile "${OUTFILE}"
InstallDir "$LOCALAPPDATA\Programs\${APP_NAME}"
InstallDirRegKey HKCU "Software\${APP_NAME}" "InstallDir"

VIProductVersion "${VERSION_QUAD}"
VIAddVersionKey "ProductName" "${APP_NAME}"
VIAddVersionKey "FileDescription" "${APP_NAME} installer"
VIAddVersionKey "FileVersion" "${VERSION}"
VIAddVersionKey "ProductVersion" "${VERSION}"
VIAddVersionKey "LegalCopyright" "MIT OR Apache-2.0"

!define MUI_ICON "..\..\assets\icon.ico"
!define MUI_UNICON "..\..\assets\icon.ico"
!define MUI_FINISHPAGE_RUN "$INSTDIR\${APP_EXE}"

!insertmacro MUI_PAGE_DIRECTORY
!insertmacro MUI_PAGE_INSTFILES
!insertmacro MUI_PAGE_FINISH
!insertmacro MUI_UNPAGE_CONFIRM
!insertmacro MUI_UNPAGE_INSTFILES
!insertmacro MUI_LANGUAGE "English"

Function WaitForUnlock
	StrCpy $R0 0
	loop:
		IfFileExists "$INSTDIR\${APP_EXE}" 0 done
		ClearErrors
		Delete "$INSTDIR\${APP_EXE}"
		IfErrors 0 done
		IntOp $R0 $R0 + 1
		IntCmp $R0 20 done 0 done
		Sleep 500
		Goto loop
	done:
FunctionEnd

Section "Install"
	SetShellVarContext current
	SetOutPath "$INSTDIR"
	Call WaitForUnlock
	File "${BINARY}"
	File "..\..\LICENSE-MIT"
	File "..\..\LICENSE-APACHE"
	WriteUninstaller "$INSTDIR\uninstall.exe"

	CreateShortcut "${SHORTCUT}" "$INSTDIR\${APP_EXE}"
!ifdef HAVE_WINSHELL
	WinShell::SetLnkAUMI "${SHORTCUT}" "${APP_ID}"
	Pop $0
!endif

	WriteRegStr HKCU "Software\${APP_NAME}" "InstallDir" "$INSTDIR"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayName" "${APP_NAME}"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayVersion" "${VERSION}"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "Publisher" "${APP_NAME}"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "InstallLocation" "$INSTDIR"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "DisplayIcon" "$INSTDIR\${APP_EXE}"
	WriteRegStr HKCU "${UNINSTALL_KEY}" "UninstallString" '"$INSTDIR\uninstall.exe"'
	WriteRegStr HKCU "${UNINSTALL_KEY}" "QuietUninstallString" '"$INSTDIR\uninstall.exe" /S'
	WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoModify" 1
	WriteRegDWORD HKCU "${UNINSTALL_KEY}" "NoRepair" 1
	${GetSize} "$INSTDIR" "/S=0K" $0 $1 $2
	IntFmt $0 "0x%08X" $0
	WriteRegDWORD HKCU "${UNINSTALL_KEY}" "EstimatedSize" "$0"

	${GetParameters} $0
	ClearErrors
	${GetOptions} $0 "/R" $1
	${IfNot} ${Errors}
	${AndIf} ${Silent}
		Exec '"$INSTDIR\${APP_EXE}"'
	${EndIf}
SectionEnd

Section "Uninstall"
	SetShellVarContext current
	Delete "$INSTDIR\${APP_EXE}"
	Delete "$INSTDIR\LICENSE-MIT"
	Delete "$INSTDIR\LICENSE-APACHE"
	Delete "$INSTDIR\uninstall.exe"
	RMDir "$INSTDIR"
	Delete "${SHORTCUT}"
	DeleteRegKey HKCU "${UNINSTALL_KEY}"
	DeleteRegKey HKCU "Software\${APP_NAME}"
SectionEnd
