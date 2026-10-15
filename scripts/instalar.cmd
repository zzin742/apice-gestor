@echo off
rem Apice Gestor - instalacao de UM CLIQUE (bancada e rollout).
rem Coloque este arquivo, o instalar.ps1, o apice-gestor_*_x64.msi e o SHA256SUMS.txt na mesma pasta
rem e de dois cliques. O Windows pede permissao de administrador UMA vez; o resto e automatico:
rem confere o hash, cria a excecao do Defender (so a pasta do app), instala, sobe o servico e enrola.
rem
rem ROLLOUT: copie a pasta da pessoa (do T:) para o disco C: da maquina dela e rode DALI (nao do T:,
rem que o processo de administrador nao enxerga). Se existir um  codigo.txt  na pasta, o codigo e lido
rem dele (um por pessoa, ja embutido) e nao pergunta nada. Sem codigo.txt, ele pergunta na janela.
rem
rem O codigo nunca vai pra log nem pra linha de comando: o instalar.ps1 o recebe pela variavel
rem APICE_CODIGO (que ele le, valida e apaga) e so o ultimo salto (apice-gestor.exe --enrolar) o recebe
rem como argumento, em processo elevado, por segundos. Nunca grave um codigo NESTE arquivo.
setlocal

rem 1. Administrador? Se nao, relanca a si mesmo elevado (UAC) e sai.
rem    (goto em vez de bloco if: um caminho com parenteses quebraria o bloco)
net session >nul 2>&1
if not errorlevel 1 goto elevado
powershell.exe -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs -WorkingDirectory '%~dp0'"
exit /b
:elevado

rem 2. Codigo: embutido (codigo.txt) ou perguntado uma vez. O instalar.ps1 faz Trim e valida o formato.
set "APICE_CODIGO="
if exist "%~dp0codigo.txt" set /p "APICE_CODIGO="<"%~dp0codigo.txt"
if not defined APICE_CODIGO set /p "APICE_CODIGO=Codigo de enrolamento (de /admin/dispositivos): "
if not defined APICE_CODIGO (
  echo Sem codigo: nada foi feito.
  pause
  exit /b 2
)

rem 3. Tudo numa execucao: o script le APICE_CODIGO do ambiente e o apaga.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0instalar.ps1"
set "SAIDA=%ERRORLEVEL%"
set "APICE_CODIGO="

rem 4. Higiene: apaga o codigo embutido desta copia local (uso-unico; depois do enroll ja nao vale).
del "%~dp0codigo.txt" >nul 2>&1

echo.
if "%SAIDA%"=="0" (echo PRONTO: instalado, servico rodando e maquina enrolada. A capsula pisca uma vez quando o servico reinicia: normal.) else (echo NAO concluiu ^(saida %SAIDA%^). Veja a mensagem acima e o log em %ProgramData%\ApiceGestor\instalar.log)
pause
exit /b %SAIDA%
