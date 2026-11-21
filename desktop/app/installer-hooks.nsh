; Hooks do instalador NSIS (tauri.bundle.conf.json). O daemon live-cmd-termd roda de uma cópia em
; $LOCALAPPDATA\live-cmd\daemon\<versão>\ e segue vivo quando o app fecha ou atualiza; só a desinstalação o encerra.

!macro NSIS_HOOK_PREUNINSTALL
  ; Encerra o daemon (sessões e programas dos terminais) antes de apagar os arquivos.
  ExecWait '"$INSTDIR\live-cmd-termd.exe" --shutdown'
  RMDir /r "$LOCALAPPDATA\live-cmd\daemon"
  RMDir /r "$LOCALAPPDATA\live-cmd\termd"
!macroend
