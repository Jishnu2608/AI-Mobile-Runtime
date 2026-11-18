; NSIS hooks for the per-user installer (REL-001).
; The Runtime Core (umr-core.exe) runs in the background and the preview window reconnects to
; it (starting it again if it is gone). Before files are replaced or removed: close the desktop
; app first, then stop the Core cleanly (`umr stop` also stops its emulators), then end any Core
; that is still running, or its file stays locked and the update is only half applied.

!macro UMR_STOP_ALL
  nsExec::Exec 'taskkill /F /IM umr-desktop.exe'
  IfFileExists "$INSTDIR\umr.exe" 0 +2
    nsExec::Exec '"$INSTDIR\umr.exe" stop'
  nsExec::Exec 'taskkill /F /IM umr-core.exe'
  ; UPD-003: other umr.exe processes from this folder (the VS Code extension's `umr rpc` bridge,
  ; agents' `umr mcp` servers) keep umr.exe locked, and the silent installer would leave the old
  ; one in place. They restart on their own after the update. Only this folder's copy is ended;
  ; the updater itself has exited by now (it starts the installer and returns).
  Sleep 1000
  nsExec::Exec `powershell -NoProfile -NonInteractive -Command "Get-Process umr -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq '$INSTDIR\umr.exe' } | Stop-Process -Force"`
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UMR_STOP_ALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UMR_STOP_ALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; DEV-003b: the emulator always writes its console token to %USERPROFILE% (it cannot be moved).
  ; The Core leaves a marker in its data folder when its own emulator start created that file;
  ; only then is the token removed (Android Studio shares it otherwise). UMR_HOME moves the data
  ; folder, so both places are checked.
  ReadEnvStr $0 UMR_HOME
  StrCmp $0 "" 0 +2
    StrCpy $0 "$PROFILE\.umr"
  IfFileExists "$0\emulator-console-token.created" 0 umr_token_done
    Delete "$PROFILE\.emulator_console_auth_token"
    Delete "$0\emulator-console-token.created"
  umr_token_done:
  ; User data is kept on purpose: emulator images, AVDs and toolchains can be many GB and
  ; may be shared with other projects. Say where they are (silent uninstalls skip this).
  IfSilent +2
    MessageBox MB_OK|MB_ICONINFORMATION "AI Mobile Runtime was removed.$\r$\n$\r$\nYour data was kept: settings and logs in %USERPROFILE%\.umr, and the toolchains / emulator images in the folder you chose (by default also inside %USERPROFILE%\.umr). Delete those folders yourself if you no longer need them."
!macroend
