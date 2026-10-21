; Replacing an executable at the same path can leave Explorer displaying its old icon.
; Notify the shell after upgrade without restarting Explorer or deleting global caches.
!macro NSIS_HOOK_POSTINSTALL
  System::Call 'shell32::SHChangeNotify(i 0x08000000, i 0, p 0, p 0)'
  IfFileExists "$DESKTOP\${PRODUCTNAME}.lnk" 0 +2
    System::Call 'shell32::SHChangeNotify(i 0x00002000, i 0x1005, w "$DESKTOP\${PRODUCTNAME}.lnk", p 0)'
  IfFileExists "$SMPROGRAMS\${PRODUCTNAME}.lnk" 0 +2
    System::Call 'shell32::SHChangeNotify(i 0x00002000, i 0x1005, w "$SMPROGRAMS\${PRODUCTNAME}.lnk", p 0)'
!macroend
