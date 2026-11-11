; SonicScript's additions to Tauri's NSIS installer (M2 phase 7, D-052). Tauri's own template does the rest:
; per-user install into %LOCALAPPDATA%\SonicScript, the "SonicScript" Start menu shortcut, and on uninstall
; its files, the shortcut and the HKCU Run value "SonicScript" (it never deletes the folder's other files).

!define SONIC_RUN "Software\Microsoft\Windows\CurrentVersion\Run"

; Start with Windows on (D-047): the exact text autostart::command_line writes, so the tray's tick shows it.
; Not in update mode: an update keeps the user's choice.
!macro NSIS_HOOK_POSTINSTALL
  ${If} $UpdateMode <> 1
    WriteRegStr HKCU "${SONIC_RUN}" "SonicScript" '"$INSTDIR\${MAINBINARYNAME}.exe"'
  ${EndIf}
!macroend

; After Tauri removed its files: ask before deleting the user's data and keys. Default No; a silent uninstall
; (also the one a reinstall runs) and an update keep everything.
!macro NSIS_HOOK_POSTUNINSTALL
  ${If} $UpdateMode <> 1
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
