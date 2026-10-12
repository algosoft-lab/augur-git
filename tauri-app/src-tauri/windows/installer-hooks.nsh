; CLI setup shared by interactive installs, upgrades, and package removal.
!include "nsDialogs.nsh"

Var AugurCliCheckbox
Var AugurCliEnabled
Var AugurCliPreviousInstall

!macro AUGUR_CLI_PAGE
  Page custom AugurCliPage AugurCliLeave
!macroend

!macro AUGUR_CLI_FUNCTIONS
Function AugurCliInitialize
  ; A clean install opts in. Existing installs keep their recorded choice;
  ; installs from versions predating the CLI default to disabled.
  ReadRegStr $AugurCliPreviousInstall SHCTX "${MANUPRODUCTKEY}" ""
  StrCpy $AugurCliEnabled "0"
  ${If} $AugurCliPreviousInstall == ""
    StrCpy $AugurCliEnabled "1"
  ${EndIf}

  ClearErrors
  ReadRegDWORD $0 HKCU "Software\com.augur.git.tauri\Cli" "Enabled"
  ${IfNot} ${Errors}
    StrCpy $AugurCliEnabled $0
  ${EndIf}
FunctionEnd

Function AugurCliPage
  ${If} $PassiveMode = 1
  ${OrIf} $UpdateMode = 1
  ${OrIf} ${Silent}
    Abort
  ${EndIf}
  !insertmacro MUI_HEADER_TEXT "Terminal command" "Add agit to your user PATH"
  nsDialogs::Create 1018
  Pop $0
  ${If} $0 == error
    Abort
  ${EndIf}

  ${NSD_CreateLabel} 0 0 100% 22u "The agit command opens repositories from PowerShell, Command Prompt, and Git Bash."
  Pop $0
  ${NSD_CreateCheckbox} 0 30u 100% 12u "Add agit to your user PATH"
  Pop $AugurCliCheckbox
  ${If} $AugurCliEnabled = 1
    ${NSD_Check} $AugurCliCheckbox
  ${EndIf}
  ${NSD_CreateLabel} 0 54u 100% 22u "You can change this later in Settings > General. Open a new terminal after changing PATH."
  Pop $0
  nsDialogs::Show
FunctionEnd

Function AugurCliLeave
  ${NSD_GetState} $AugurCliCheckbox $0
  StrCpy $AugurCliEnabled $0
FunctionEnd
!macroend

!macro NSIS_HOOK_POSTINSTALL
  ${If} $AugurCliEnabled = 1
    nsExec::ExecToLog '"$INSTDIR\bin\agit.exe" --internal-cli-register'
  ${Else}
    nsExec::ExecToLog '"$INSTDIR\bin\agit.exe" --internal-cli-unregister'
  ${EndIf}
  Pop $0
  ${If} $0 != 0
    DetailPrint "Could not update the agit user PATH registration (exit status: $0)."
    ${If} $PassiveMode != 1
      IfSilent augur_cli_install_warning_done 0
      MessageBox MB_ICONEXCLAMATION|MB_OK "Augur Git was installed, but the agit PATH registration failed. You can retry from Settings > General."
      augur_cli_install_warning_done:
    ${EndIf}
  ${EndIf}
!macroend

!macro NSIS_HOOK_PREUNINSTALL
  ${If} $UpdateMode != 1
    nsExec::ExecToLog '"$INSTDIR\bin\agit.exe" --internal-cli-uninstall-package'
    Pop $0
    ${If} $0 != 0
      DetailPrint "Could not remove the agit user PATH registration (exit status: $0)."
      ${If} $PassiveMode != 1
        IfSilent augur_cli_uninstall_warning_done 0
        MessageBox MB_ICONEXCLAMATION|MB_OK "Augur Git could not remove its agit PATH registration. Remove the Augur Git bin entry from your user PATH after uninstalling."
        augur_cli_uninstall_warning_done:
      ${EndIf}
    ${EndIf}
  ${EndIf}
!macroend
