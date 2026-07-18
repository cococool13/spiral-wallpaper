; Spiral Wallpaper — NSIS install-flow copy.
; Brand rule: state what actually happens, never invent steps. Every line
; below describes a real action the installer takes, in the order it happens.

!macro NSIS_HOOK_PREINSTALL
  DetailPrint "Setting up Spiral Wallpaper..."
  DetailPrint "Copying files to your machine..."
!macroend

!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Spiral is linked to your Start Menu."
  DetailPrint "Verifying the install..."
  IfFileExists "$INSTDIR\${MAINBINARYNAME}.exe" +3 0
    DetailPrint "Verification failed: ${MAINBINARYNAME}.exe is missing. Re-run this installer."
    Abort "Verification failed: ${MAINBINARYNAME}.exe is missing from $INSTDIR."
  DetailPrint "Verified."
  DetailPrint "Done. Nothing else happens until you open it."
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  DetailPrint "Removing Spiral Wallpaper..."
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Spiral is removed. Nothing was left running."
!macroend
