; NSIS hooks for the per-user installer (REL-001).
; The Runtime Core (tumr-core.exe) runs in the background and the preview window reconnects to
; it (starting it again if it is gone). Before files are replaced or removed: close the desktop
; app first, then stop the Core cleanly (`tumr stop` also stops its emulators), then end any Core
; that is still running, or its file stays locked and the update is only half applied.

!macro TUMR_STOP_ALL
  nsExec::Exec 'taskkill /F /IM tumr-desktop.exe'
  IfFileExists "$INSTDIR\tumr.exe" 0 +2
    nsExec::Exec '"$INSTDIR\tumr.exe" stop'
  nsExec::Exec 'taskkill /F /IM tumr-core.exe'
  ; UPD-003: other tumr.exe processes from this folder (the VS Code extension's `tumr rpc` bridge,
  ; agents' `tumr mcp` servers) keep tumr.exe locked, and the silent installer would leave the old
  ; one in place. They restart on their own after the update. Only this folder's copy is ended;
  ; the updater itself has exited by now (it starts the installer and returns).
  ; The installer is 32-bit, so plain `powershell` is the 32-bit one, which cannot read the path
  ; of a 64-bit process (`Path` is empty): the filter never matched and tumr.exe stayed locked
  ; (live update to 0.1.29, 2026-11-19). Sysnative reaches the 64-bit PowerShell.
  Sleep 1000
  IfFileExists "$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" 0 tumr_ps_plain
    nsExec::Exec `"$WINDIR\Sysnative\WindowsPowerShell\v1.0\powershell.exe" -NoProfile -NonInteractive -Command "Get-Process tumr -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq '$INSTDIR\tumr.exe' } | Stop-Process -Force"`
    Goto tumr_ps_done
  tumr_ps_plain:
    nsExec::Exec `powershell -NoProfile -NonInteractive -Command "Get-Process tumr -ErrorAction SilentlyContinue | Where-Object { $$_.Path -eq '$INSTDIR\tumr.exe' } | Stop-Process -Force"`
  tumr_ps_done:
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro TUMR_STOP_ALL
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ; RENAME-001 (2026-11-20): the product was installed as "AI Mobile Runtime" before. Its own
  ; uninstaller runs silently and in place (`_?=`, so this waits for it): it stops the old Core and
  ; emulators, takes its folder off the PATH and removes its files, Start menu entry and registry
  ; entry. The data folder (.umr) and the toolchains stay; the first `tumr` run below takes the
  ; data folder over (.tumr) and the agents are moved to the new name (`agent repoint`).
  StrCmp "$INSTDIR" "$LOCALAPPDATA\AI Mobile Runtime" tumr_no_legacy
  IfFileExists "$LOCALAPPDATA\AI Mobile Runtime\uninstall.exe" 0 tumr_no_legacy
    ExecWait '"$LOCALAPPDATA\AI Mobile Runtime\uninstall.exe" /S _?=$LOCALAPPDATA\AI Mobile Runtime'
    Delete "$LOCALAPPDATA\AI Mobile Runtime\uninstall.exe"
    RMDir "$LOCALAPPDATA\AI Mobile Runtime"
  tumr_no_legacy:
  ; W9 / DEC-042: the install folder goes on the user PATH only if the user chose it earlier
  ; (`tumr path add` or the app's checkbox); off by default. An update that ran the uninstaller
  ; took it out; this puts it back.
  nsExec::Exec '"$INSTDIR\tumr.exe" path restore'
  ; DEC-055: agent configs hold tumr.exe's absolute path; point all-projects entries here
  nsExec::Exec '"$INSTDIR\tumr.exe" agent repoint'
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro TUMR_STOP_ALL
  ; W9: take the folder off the PATH while tumr.exe still exists; the choice is kept, so a
  ; reinstall or update puts it back (`path restore` above)
  IfFileExists "$INSTDIR\tumr.exe" 0 +2
    nsExec::Exec '"$INSTDIR\tumr.exe" path remove --keep-choice'
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; DEV-003b: the emulator always writes its console token to %USERPROFILE% (it cannot be moved).
  ; The Core leaves a marker in its data folder when its own emulator start created that file;
  ; only then is the token removed (Android Studio shares it otherwise). TUMR_HOME moves the data
  ; folder, so both places are checked.
  ReadEnvStr $0 TUMR_HOME
  StrCmp $0 "" 0 +2
    StrCpy $0 "$PROFILE\.tumr"
  IfFileExists "$0\emulator-console-token.created" 0 tumr_token_done
    Delete "$PROFILE\.emulator_console_auth_token"
    Delete "$0\emulator-console-token.created"
  tumr_token_done:
  ; User data is kept on purpose: emulator images, AVDs and toolchains can be many GB and
  ; may be shared with other projects. Say where they are (silent uninstalls skip this).
  IfSilent +2
    MessageBox MB_OK|MB_ICONINFORMATION "Toolchain Universal Mobile Runtime was removed.$\r$\n$\r$\nYour data was kept: settings and logs in %USERPROFILE%\.tumr, and the toolchains / emulator images in the folder you chose (by default also inside %USERPROFILE%\.tumr). Delete those folders yourself if you no longer need them."
!macroend
