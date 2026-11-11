; SonicScript's additions to Tauri's NSIS installer (M2 phase 7, D-052). Tauri's own template does the rest:
; per-user install into %LOCALAPPDATA%\SonicScript, the "SonicScript" Start menu shortcut, and on uninstall
; its files, the shortcut and the HKCU Run value "SonicScript" (it never deletes the folder's other files).
;
; A reinstall or upgrade first runs the old uninstaller in place ("uninstall.exe _?=<folder>", not silent):
; then $EXEDIR is the install folder. Started from Settings > Apps, an NSIS uninstaller runs from a temp copy.

!define SONIC_RUN "Software\Microsoft\Windows\CurrentVersion\Run"
; Carries "start with Windows" from the old install to the new one: "1" on, "0" off (deleted after use).
!define SONIC_KEY "Software\SonicScript\Installer"

; A first install turns start with Windows on (D-047): no Apps entry (a real uninstall deletes it; Tauri's
; install-location key outlives it, so it cannot tell) and no note from a reinstall.
!macro NSIS_HOOK_PREINSTALL
  ReadRegStr $0 SHCTX "${UNINSTKEY}" "UninstallString"
  ReadRegStr $1 HKCU "${SONIC_KEY}" "StartWithWindows"
  ${If} $0 == ""
  ${AndIf} $1 == ""
    WriteRegStr HKCU "${SONIC_KEY}" "StartWithWindows" "1"
  ${EndIf}
!macroend

; The Run value: as the old install had it (or on for a first install). An upgrade that skipped the old
; uninstaller leaves no note: the value is untouched. The text is what autostart::command_line writes, so the
; tray's tick shows it.
!macro NSIS_HOOK_POSTINSTALL
  ReadRegStr $0 HKCU "${SONIC_KEY}" "StartWithWindows"
  ${If} $0 == "1"
    WriteRegStr HKCU "${SONIC_RUN}" "SonicScript" '"$INSTDIR\${MAINBINARYNAME}.exe"'
  ${EndIf}
  DeleteRegValue HKCU "${SONIC_KEY}" "StartWithWindows"
  DeleteRegKey /ifempty HKCU "${SONIC_KEY}"
!macroend

; Before Tauri deletes the Run value: during a reinstall, note whether it was on.
!macro NSIS_HOOK_PREUNINSTALL
  ${If} $EXEDIR == $INSTDIR
    ReadRegStr $0 HKCU "${SONIC_RUN}" "SonicScript"
    ${If} $0 == ""
      WriteRegStr HKCU "${SONIC_KEY}" "StartWithWindows" "0"
    ${Else}
      WriteRegStr HKCU "${SONIC_KEY}" "StartWithWindows" "1"
    ${EndIf}
  ${EndIf}
!macroend

; After Tauri removed its files, a real uninstall (not a reinstall, not an update) asks before deleting the
; user's data and keys. Default No; a silent uninstall keeps everything.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
  ${AndIf} $EXEDIR != $INSTDIR
    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 \
      "Also delete your SonicScript history (recordings and texts), settings and word list?$\r$\n$\r$\nChoose No to keep them for a later install." \
      /SD IDNO IDNO sonic_keep_data
    SetShellVarContext current
    RMDir /r "$LOCALAPPDATA\SonicScript"
    RMDir /r "$APPDATA\SonicScript"
    sonic_keep_data:

    MessageBox MB_YESNO|MB_ICONQUESTION|MB_DEFBUTTON2 \
      "Also delete the API keys SonicScript saved in Windows Credential Manager?$\r$\n$\r$\nKeys you set as environment variables are not touched." \
      /SD IDNO IDNO sonic_keep_keys
    nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:SonicScript/GEMINI_API_KEY'
    Pop $0
    nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:SonicScript/GROQ_API_KEY'
    Pop $0
    nsExec::Exec '"$SYSDIR\cmdkey.exe" /delete:SonicScript/OPENAI_API_KEY'
    Pop $0
    sonic_keep_keys:
  ${EndIf}
!macroend
