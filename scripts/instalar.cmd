@echo off
rem Apice Gestor - instalacao de UM CLIQUE (bancada e rollout).
rem Coloque este arquivo, o instalar.ps1, o apice-gestor_*_x64.msi e o SHA256SUMS.txt na mesma pasta
rem e de dois cliques. O Windows pede permissao de administrador UMA vez; depois ele pede o codigo de
rem enrolamento (gerado em /admin/dispositivos) UMA vez, e o resto e automatico: confere o hash,
rem cria a excecao do Defender (so a pasta do app), instala, sobe o servico e enrola.
rem
rem O codigo NUNCA vai em linha de comando nem em historico: este arquivo se eleva ANTES de pedir o
rem codigo e o entrega ao instalar.ps1 pela variavel de ambiente APICE_CODIGO, que o script le e
rem apaga. Nunca grave um codigo dentro deste arquivo.
setlocal

rem 1. Administrador? Se nao, relanca a si mesmo elevado (UAC) e sai.
rem    (goto em vez de bloco if: um caminho com parenteses quebraria o bloco)
net session >nul 2>&1
if not errorlevel 1 goto elevado
powershell.exe -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs -WorkingDirectory '%~dp0'"
exit /b
:elevado

rem 2. Ja elevado: pede o codigo uma vez.
set "APICE_CODIGO="
set /p "APICE_CODIGO=Codigo de enrolamento (de /admin/dispositivos): "
if "%APICE_CODIGO%"=="" (
  echo Sem codigo: nada foi feito.
  pause
  exit /b 2
)

rem 3. Tudo numa execucao: o script le APICE_CODIGO do ambiente e o apaga.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0instalar.ps1"
set "SAIDA=%ERRORLEVEL%"
set "APICE_CODIGO="
echo.
if "%SAIDA%"=="0" (echo PRONTO: instalado, servico rodando e maquina enrolada. A capsula pisca uma vez quando o servico reinicia: normal.) else (echo NAO concluiu ^(saida %SAIDA%^). Veja a mensagem acima e o log em %ProgramData%\ApiceGestor\instalar.log)
pause
exit /b %SAIDA%
