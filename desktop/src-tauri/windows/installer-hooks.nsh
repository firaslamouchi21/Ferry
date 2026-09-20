!macro NSIS_HOOK_POSTINSTALL
  DetailPrint "Adding a Windows Firewall exception for the Ferry daemon (needed for LAN peer discovery and transfer)..."
  nsExec::ExecToLog '"netsh" advfirewall firewall add rule name="Ferry daemon (inbound)" dir=in action=allow program="$INSTDIR\ferry-daemon.exe" enable=yes profile=any'
  Pop $0
  nsExec::ExecToLog '"netsh" advfirewall firewall add rule name="Ferry daemon (outbound)" dir=out action=allow program="$INSTDIR\ferry-daemon.exe" enable=yes profile=any'
  Pop $0
!macroend

!macro NSIS_HOOK_POSTUNINSTALL
  DetailPrint "Removing the Ferry daemon's Windows Firewall exception..."
  nsExec::ExecToLog '"netsh" advfirewall firewall delete rule name="Ferry daemon (inbound)"'
  Pop $0
  nsExec::ExecToLog '"netsh" advfirewall firewall delete rule name="Ferry daemon (outbound)"'
  Pop $0
!macroend
