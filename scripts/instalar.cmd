@echo off
rem Apice Gestor - instalacao de UM CLIQUE (bancada e rollout).
rem Coloque este arquivo, o instalar.ps1, o apice-gestor_*_x64.msi e o SHA256SUMS.txt na mesma pasta
rem e de dois cliques. Ele pede o codigo de enrolamento (gerado em /admin/dispositivos) UMA vez; o
rem Windows pede permissao de administrador UMA vez; o resto e automatico: instala, cria a excecao
rem do Defender (so a pasta do app), sobe o servico e enrola.
rem Tambem aceita o codigo como argumento: instalar.cmd ABCD-1234
rem Nunca grave um codigo dentro deste arquivo.
setlocal
set "COD=%~1"
if "%COD%"=="" set /p "COD=Codigo de enrolamento (de /admin/dispositivos): "
if "%COD%"=="" (
  echo Sem codigo: nada foi feito.
  pause
  exit /b 2
)
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0instalar.ps1" -Codigo "%COD%"
set "SAIDA=%ERRORLEVEL%"
set "COD="
echo.
if "%SAIDA%"=="0" (echo PRONTO: instalado, servico rodando e maquina enrolada.) else (echo NAO concluiu ^(saida %SAIDA%^). Veja a mensagem acima e o log em %ProgramData%\ApiceGestor\instalar.log)
pause
exit /b %SAIDA%
