@echo off
rem Apice Gestor - instalacao de UM CLIQUE (bancada e rollout).
rem Coloque este arquivo, o instalar.ps1, o apice-gestor_*_x64.msi e o SHA256SUMS.txt na mesma pasta
rem e de dois cliques. O Windows pede permissao de administrador UMA vez; o resto e automatico:
rem confere o hash, cria a excecao do Defender (so a pasta do app), instala, sobe o servico e enrola.
rem
rem CODIGO DE ENROLAMENTO (gerado em /admin/dispositivos):
rem   - Se existir um arquivo  codigo.txt  nesta pasta, o codigo e lido dele (rollout: um por pessoa,
rem     ja embutido na pasta T: de cada um; nao precisa gerar nem digitar). Senao, ele pergunta.
rem   - O codigo so pode ser lido DEPOIS da elevacao, e o processo de administrador NAO enxerga a
rem     unidade de rede T:. Por isso a pasta e primeiro copiada pra C:\Users\Public (que o admin
rem     enxerga) e o instalador roda de la. Assim o clique direto do T: funciona.
rem   - O codigo nunca vai pra log nem pra linha de comando: o instalar.ps1 o recebe pela variavel
rem     APICE_CODIGO (que ele le, valida e apaga) e so o ultimo salto (apice-gestor.exe --enrolar) o
rem     recebe como argumento, em processo elevado, por segundos. Nunca grave um codigo NESTE arquivo.
setlocal
set "STAGE=%PUBLIC%\apice-gestor-inst"

rem 1. Se ainda nao rodamos da pasta local de staging, copia tudo pra la e relanca de la.
rem    (o processo elevado nao ve a unidade de rede T:; de C:\Users\Public ele ve.)
if /i "%~dp0"=="%STAGE%\" goto jalocal
robocopy "%~dp0." "%STAGE%" /e /r:1 /w:1 /njh /njs /ndl /nfl /nc /ns /np >nul
if errorlevel 8 (
  echo Nao consegui preparar a pasta de instalacao em %STAGE%.
  pause
  exit /b 9
)
start "" "%STAGE%\instalar.cmd"
exit /b
:jalocal

rem 2. Administrador? Se nao, relanca a si mesmo elevado (UAC) e sai.
rem    (goto em vez de bloco if: um caminho com parenteses quebraria o bloco)
net session >nul 2>&1
if not errorlevel 1 goto elevado
powershell.exe -NoProfile -Command "Start-Process -FilePath '%~f0' -Verb RunAs -WorkingDirectory '%~dp0'"
exit /b
:elevado

rem 3. Codigo: embutido (codigo.txt) ou perguntado uma vez. O instalar.ps1 faz Trim e valida o formato.
set "APICE_CODIGO="
if exist "%~dp0codigo.txt" set /p "APICE_CODIGO="<"%~dp0codigo.txt"
if not defined APICE_CODIGO set /p "APICE_CODIGO=Codigo de enrolamento (de /admin/dispositivos): "
if not defined APICE_CODIGO (
  echo Sem codigo: nada foi feito.
  pause
  exit /b 2
)

rem 4. Tudo numa execucao: o script le APICE_CODIGO do ambiente e o apaga.
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0instalar.ps1"
set "SAIDA=%ERRORLEVEL%"
set "APICE_CODIGO="

rem 5. Higiene: apaga o codigo embutido desta copia local (uso-unico; depois do enroll ja nao vale).
del "%~dp0codigo.txt" >nul 2>&1

echo.
if "%SAIDA%"=="0" (echo PRONTO: instalado, servico rodando e maquina enrolada. A capsula pisca uma vez quando o servico reinicia: normal.) else (echo NAO concluiu ^(saida %SAIDA%^). Veja a mensagem acima e o log em %ProgramData%\ApiceGestor\instalar.log)
pause
exit /b %SAIDA%
