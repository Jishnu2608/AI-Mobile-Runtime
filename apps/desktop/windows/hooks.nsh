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
  Sleep 500
!macroend

!macro NSIS_HOOK_PREINSTALL
  !insertmacro UMR_STOP_ALL
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  !insertmacro UMR_STOP_ALL
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  ; User data is kept on purpose: emulator images, AVDs and toolchains can be many GB and
  ; may be shared with other projects. Say where they are (silent uninstalls skip this).
  IfSilent +2
    MessageBox MB_OK|MB_ICONINFORMATION "AI Mobile Runtime was removed.$\r$\n$\r$\nYour data was kept: settings and logs in %USERPROFILE%\.umr, and the toolchains / emulator images in the folder you chose (by default also inside %USERPROFILE%\.umr). Delete those folders yourself if you no longer need them."
!macroend
