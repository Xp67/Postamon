; Postamon si registra da solo fra i programmi per i link email (mailto) a ogni
; avvio (src-tauri/src/mailto.rs): qui, quando lo si disinstalla, si toglie la
; registrazione, cosi' Windows non propone piu' un programma che non c'e'.

!macro NSIS_HOOK_POSTUNINSTALL
  DeleteRegKey HKCU "Software\Classes\${PRODUCTNAME}.mailto"
  DeleteRegKey HKCU "Software\${PRODUCTNAME}\Capabilities"
  DeleteRegKey /ifempty HKCU "Software\${PRODUCTNAME}"
  DeleteRegValue HKCU "Software\RegisteredApplications" "${PRODUCTNAME}"
!macroend
