<#
.SYNOPSIS
  Instala (ou desinstala) o Ápice Gestor num PC e enrola a máquina no Hub. Presencial, uma vez por PC.

.DESCRIPTION
  Rode COMO O COLABORADOR (a sessão dele aberta), sem "Executar como administrador":

      powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1
      powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Desinstalar

  UM CLIQUE (bancada e rollout): dois cliques no instalar.cmd ao lado do .msi. Ele se eleva (UAC),
  pede o código UMA vez e chama este script, que numa só execução elevada instala o .msi, cria a
  exceção no Defender (só a pasta do app), sobe o serviço e enrola com o código (uma tentativa,
  sem perguntar mais nada).

  O CÓDIGO NUNCA VAI PARA LOG NEM HISTÓRICO: o instalar.cmd o entrega pela variável de ambiente
  APICE_CODIGO (lida e apagada aqui), não pela linha de comando. -Codigo existe para automação,
  mas, digitado no PowerShell, fica no histórico do PSReadLine
  (%APPDATA%\Microsoft\Windows\PowerShell\PSReadLine\ConsoleHost_history.txt): prefira o
  instalar.cmd, ou apague a linha desse arquivo depois. Rodando sem elevação com -Codigo, o código
  também vai na linha de comando da instância elevada (visível só para administradores).

  Instalar tem três partes:
    1. Máquina (administrador; o script pede o UAC sozinho): WebView2 se faltar, o .msi em
       silêncio e a exceção no Defender. Pode ser autorizada com a senha da TI.
    2. Enrolamento (na MESMA etapa elevada): pede o código gerado no Hub e chama
       "apice-gestor.exe --enrolar <código>". O token vai para o cofre DA MÁQUINA
       (%ProgramData%\ApiceGestor\device.token, DPAPI de máquina, só SYSTEM e Administradores);
       quem o lê é o serviço. Pode ser autorizado pela conta da TI.
    3. Início automático: com o serviço (builds da feat/servico), é ele que abre a cápsula em cada
       logon, e o script remove o HKCU\Run legado. Sem serviço (builds antigos), registra o Run do
       colaborador, respeitando quem desativou no Gerenciador de Tarefas.

  Idempotente: não reinstala o mesmo .msi (mesmo ProductCode), não duplica a exceção e não enrola
  de novo quem já tem token (-Reenrolar para trocar).

  Rate limit do POST /enroll (10/min por IP; os 95 PCs saem pelo mesmo IP):
    - 429: o executável já espera retry_after_seconds (até 3x, teto 300 s);
    - código inválido/expirado/usado: NUNCA é reenviado, nem se digitarem de novo; pede código novo;
    - no máximo UMA chamada automática por código; repetir é sempre decisão de quem instala.

  Desinstalar remove o app, a exceção no Defender e o início automático (a marca de "desativado"
  do Gerenciador de Tarefas fica, para valer numa reinstalação) e PRESERVA o token (reinstalar não gasta outro
  código). -RemoverToken apaga o token também.

  Não precisa de domínio nem de Microsoft 365. Não grava código nem token em log.

.PARAMETER Msi
  Caminho do .msi. Padrão: o único apice-gestor_*_x64.msi da pasta do script. Se houver
  SHA256SUMS.txt ao lado, o hash é conferido antes de instalar.

.PARAMETER Reenrolar
  Enrola de novo mesmo com token presente (ex.: dispositivo revogado no Hub).

.PARAMETER SemEnrolar
  Só a etapa da máquina.

.PARAMETER Desinstalar
  Desinstala. Preserva o token, a não ser com -RemoverToken.

.NOTES
  Salvo em UTF-8 COM BOM: sem BOM o Windows PowerShell 5.1 lê como ANSI e estraga os acentos.
  Saídas: 0 ok · 1 erro na etapa da máquina · 2 código recusado (inválido/expirado/usado, ou
  formato inválido) · 3 limite de tentativas · 4 sem conexão com o Hub · 5 outro erro no enrolamento.
#>
[CmdletBinding()]
param(
    [string]$Msi,
    [switch]$Reenrolar,
    [switch]$SemEnrolar,
    [switch]$Desinstalar,
    [switch]$RemoverToken,
    # Um clique: código de enrolamento de /admin/dispositivos. Uma tentativa só, sem perguntar nada.
    # NUNCA escrever um código em arquivo do repo: é parâmetro de quem instala.
    [string]$Codigo,
    # Interno: o script se relança elevado só para a parte que precisa de administrador.
    [ValidateSet('Tudo', 'Maquina', 'Remocao')]
    [string]$Etapa = 'Tudo'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

# --- Constantes ------------------------------------------------------------------------------
# Iguais às do src-tauri (tauri.conf.json e api.rs). Mudou lá, muda aqui.
$UpgradeCode     = '{97800A68-CABD-52E3-886A-6D8831965CFF}'
$NomeApp         = [char]0x00C1 + 'pice Gestor'                       # "Ápice Gestor"
$PastaApp        = Join-Path $env:ProgramFiles $NomeApp
$Exe             = Join-Path $PastaApp 'apice-gestor.exe'
$Processo        = 'apice-gestor'
# Serviço do Windows (demanda de bloqueio, opção B, 09/10). Quem cria/remove é o .msi (fragmento
# WiX); o script confere. Builds sem serviço (os do piloto até aqui) seguem o caminho antigo.
$Servico         = 'ApiceGestor'
$KeychainServico = 'br.com.apice.gestor'                              # alvo no Credential Manager
$WebView2Id      = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$WebView2Url     = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703' # Evergreen Bootstrapper
$PastaLog        = Join-Path $env:ProgramData 'ApiceGestor'
# Cofre da máquina (feat/servico, cofre.rs): o --enrolar ELEVADO grava aqui, cifrado com DPAPI de
# máquina e com a pasta fechada para SYSTEM + Administradores; quem lê é o serviço.
$Cofre           = Join-Path $PastaLog 'device.token'
$RaizesUninstall = @(
    'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall',
    'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall'
)

# --- Utilidades ------------------------------------------------------------------------------
function Escrever([string]$texto, [string]$cor = 'Gray') {
    Write-Host $texto -ForegroundColor $cor
    try {
        if (-not (Test-Path $PastaLog)) { New-Item -ItemType Directory -Force $PastaLog | Out-Null }
        "$(Get-Date -Format s)  [$env:USERNAME]  $texto" | Add-Content -Encoding UTF8 (Join-Path $PastaLog 'instalar.log')
    } catch {
        Write-Verbose "Sem log em disco: $($_.Exception.Message)"   # sem permissão no ProgramData: só a tela
    }
}

function EhAdmin {
    $p = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
    return $p.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Com([object]$obj, [string]$membro, [string]$tipo, [object[]]$argumentos) {
    return $obj.GetType().InvokeMember($membro, $tipo, $null, $obj, $argumentos)
}

function PropriedadeDoMsi([string]$caminho, [string]$nome) {
    $wi = New-Object -ComObject WindowsInstaller.Installer
    $db = Com $wi 'OpenDatabase' 'InvokeMethod' @($caminho, 0)
    $view = Com $db 'OpenView' 'InvokeMethod' @("SELECT Value FROM Property WHERE Property='$nome'")
    Com $view 'Execute' 'InvokeMethod' $null | Out-Null
    $rec = Com $view 'Fetch' 'InvokeMethod' $null
    $valor = Com $rec 'StringData' 'GetProperty' @(1)
    Com $view 'Close' 'InvokeMethod' $null | Out-Null
    return $valor
}

# ProductCodes instalados desta família (mesmo UpgradeCode). Cada build tem ProductCode novo.
function ProdutosInstalados {
    $codigos = @()
    try {
        $wi = New-Object -ComObject WindowsInstaller.Installer
        foreach ($c in (Com $wi 'RelatedProducts' 'GetProperty' @($UpgradeCode))) { $codigos += [string]$c }
    } catch {
        Write-Verbose "COM do Windows Installer falhou; procurando no registro."
    }
    if (-not $codigos) {
        foreach ($raiz in $RaizesUninstall) {
            Get-ChildItem $raiz -ErrorAction SilentlyContinue | ForEach-Object {
                $p = Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue
                if ($p -and $p.PSObject.Properties['DisplayName'] -and $p.DisplayName -eq $NomeApp -and $_.PSChildName -like '{*}') {
                    $codigos += $_.PSChildName
                }
            }
        }
    }
    return @($codigos | Select-Object -Unique)
}

function VersaoInstalada([string]$productCode) {
    foreach ($raiz in $RaizesUninstall) {
        $p = Get-ItemProperty (Join-Path $raiz $productCode) -ErrorAction SilentlyContinue
        if ($p -and $p.PSObject.Properties['DisplayVersion']) { return [version]$p.DisplayVersion }
    }
    return $null
}

function WebView2Presente {
    $chaves = @(
        "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$WebView2Id",
        "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$WebView2Id",
        "HKCU:\SOFTWARE\Microsoft\EdgeUpdate\Clients\$WebView2Id"
    )
    foreach ($k in $chaves) {
        $pv = (Get-ItemProperty -Path $k -Name pv -ErrorAction SilentlyContinue).pv
        if ($pv -and $pv -ne '0.0.0.0') { return $true }
    }
    return $false
}

# LEGADO: alvos do Credential Manager do usuário (onde o token ficava antes do cofre da máquina).
# Só para limpeza; o app de hoje não os lê.
function AlvosDoToken {
    $alvos = @()
    foreach ($linha in (cmdkey /list 2>$null)) {
        if ($linha -match '(?:Target|Destino)\s*:\s*(.+)$' -and $Matches[1] -like "*$KeychainServico*") {
            $alvos += ($Matches[1].Trim() -replace '^LegacyGeneric:target=', '')
        }
    }
    return @($alvos)
}

# Enrolada = existe o device.token no cofre. Só administrador enxerga a pasta: chamar elevado.
function JaEnrolado { return (Test-Path -LiteralPath $Cofre) }

# O .msi traz serviço? Lê a tabela ServiceInstall do próprio pacote (ausente = não traz).
function MsiTemServico([string]$caminho) {
    try {
        $wi = New-Object -ComObject WindowsInstaller.Installer
        $db = Com $wi 'OpenDatabase' 'InvokeMethod' @($caminho, 0)
        $view = Com $db 'OpenView' 'InvokeMethod' @("SELECT Name FROM ServiceInstall WHERE Name='$Servico'")
        Com $view 'Execute' 'InvokeMethod' $null | Out-Null
        $rec = Com $view 'Fetch' 'InvokeMethod' $null
        Com $view 'Close' 'InvokeMethod' $null | Out-Null
        return ($null -ne $rec)
    } catch {
        return $false   # sem a tabela ServiceInstall
    }
}

function InfoServico {
    return Get-CimInstance Win32_Service -Filter "Name='$Servico'" -ErrorAction SilentlyContinue
}

# Confere o serviço que o .msi instalou: Automático, LocalSystem, rodando, recuperação ligada.
function ConferirServico {
    $svc = InfoServico
    if (-not $svc) { throw "O .msi traz o serviço $Servico, mas ele não foi criado. Veja o log do msiexec em $PastaLog." }
    if ($svc.StartMode -ne 'Auto') {
        Set-Service -Name $Servico -StartupType Automatic
        Escrever "Serviço: início ajustado para Automático (estava $($svc.StartMode))." 'Yellow'
    }
    if ($svc.StartName -ne 'LocalSystem') { throw "Serviço: conta $($svc.StartName), esperado LocalSystem. Não sigo." }

    # Recuperação também para saída com erro (o WiX só cobre a queda). sc.exe é idempotente.
    & sc.exe failureflag $Servico 1 | Out-Null
    $falhas = (& sc.exe qfailure $Servico | Out-String)
    if ($falhas -notmatch 'RESTART') {
        & sc.exe failure $Servico reset= 86400 actions= restart/60000/restart/60000/restart/60000 | Out-Null
        Escrever 'Serviço: recuperação configurada (reinicia em 1 min).' 'Yellow'
    }

    if ((Get-Service -Name $Servico).Status -ne 'Running') { Start-Service -Name $Servico -ErrorAction SilentlyContinue }
    try {
        (Get-Service -Name $Servico).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
    } catch {
        throw "Serviço $Servico não ficou rodando em 30 s. Veja o Visualizador de Eventos (Sistema, origem Service Control Manager)."
    }
    Escrever "Serviço: $Servico rodando (Automático, LocalSystem)." 'Green'
}

# Serviço que sobrou depois do msiexec /x (MSI antigo, falha no meio): para e apaga.
function RemoverServicoOrfao {
    $svc = InfoServico
    if (-not $svc) { return }
    Escrever "Serviço: $Servico ainda existe depois de desinstalar; removendo." 'Yellow'
    Stop-Service -Name $Servico -Force -ErrorAction SilentlyContinue
    try { (Get-Service -Name $Servico).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30)) } catch { Write-Verbose 'não parou em 30 s' }
    if ($svc.ProcessId) { Stop-Process -Id $svc.ProcessId -Force -ErrorAction SilentlyContinue }
    $saida = (& sc.exe delete $Servico | Out-String)
    if ($LASTEXITCODE -eq 1072 -or $saida -match '1072') {
        Escrever 'Serviço: marcado para exclusão; some depois de reiniciar o PC.' 'Yellow'
    } elseif (InfoServico) {
        throw "Não consegui remover o serviço $Servico (sc delete: $($saida.Trim()))."
    } else {
        Escrever "Serviço: $Servico removido." 'Green'
    }
}

function PararApp {
    # Com o serviço vivo, ele relança a cápsula em segundos, e o msiexec confere arquivos em uso
    # (InstallValidate) ANTES de parar o serviço (StopServices do ServiceControl): o apice-gestor.exe
    # estaria em uso → Restart Manager → 3010/pedido de reinício, ou falha em /qn (H3, revisão do
    # Diretor na 4/4). Por isso: PARA O SERVIÇO PRIMEIRO, pelo SCM (não conta como queda, a
    # recuperação não age), e só depois fecha as cápsulas. O MSI religa o serviço no fim
    # (ServiceControl Start=install); se o msiexec falhar, EtapaMaquina religa.
    $svc = InfoServico
    if ($svc -and $svc.State -ne 'Stopped') {
        Stop-Service -Name $Servico -Force -ErrorAction SilentlyContinue
        try {
            (Get-Service -Name $Servico).WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
        } catch {
            throw "O serviço $Servico não parou em 30 s; sem isso os arquivos ficam em uso. Rode de novo ou reinicie o PC."
        }
        Escrever 'Serviço parado para a troca de arquivos (o instalador religa no fim).'
    }
    $abertos = @(Get-Process -Name $Processo -ErrorAction SilentlyContinue)
    if ($abertos) {
        $abertos | Stop-Process -Force -ErrorAction SilentlyContinue
        Start-Sleep -Seconds 1
        Escrever "App fechado ($($abertos.Count) processo(s))."
    }
}

function RelancarElevado([string]$etapa, [string[]]$extras) {
    $argsElevado = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Etapa', $etapa) + $extras
    try {
        $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argsElevado -Verb RunAs -Wait -PassThru
    } catch {
        Write-Host 'Permissão de administrador negada. Nada foi feito na máquina.' -ForegroundColor Red
        return 1
    }
    return $p.ExitCode
}

# Exceção no Defender (pasta do app), ANTES do .msi: o Defender não pode segurar a cópia nem o
# primeiro start do app (e, depois, do serviço). Sem Defender ativo (outro antivírus): avisa e segue.
function ExcecaoDefender {
    try {
        $existentes = @((Get-MpPreference).ExclusionPath)
        if ($existentes -contains $PastaApp) {
            Escrever 'Defender: exceção já existe.'
        } else {
            Add-MpPreference -ExclusionPath $PastaApp
            Escrever "Defender: exceção adicionada para $PastaApp." 'Green'
        }
    } catch {
        Escrever "Defender: não deu para configurar ($($_.Exception.Message)). Se houver outro antivírus, adicione a exceção de $PastaApp nele." 'Yellow'
    }
}

# --- Etapa da máquina: instalar (administrador) ----------------------------------------------
function EtapaMaquina([string]$msi) {
    Escrever '== Etapa da máquina ==' 'Cyan'

    # WebView2: o .msi também baixa o bootstrapper na instalação (decisão de 08/10). Aqui só garantimos.
    if (WebView2Presente) {
        Escrever 'WebView2: já presente.'
    } else {
        Escrever 'WebView2: ausente, instalando (precisa de internet)...' 'Yellow'
        $boot = Join-Path $env:TEMP 'MicrosoftEdgeWebview2Setup.exe'
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $WebView2Url -OutFile $boot -UseBasicParsing
        $p = Start-Process -FilePath $boot -ArgumentList '/silent', '/install' -Wait -PassThru
        Remove-Item $boot -Force -ErrorAction SilentlyContinue
        if (-not (WebView2Presente)) { throw "WebView2 não instalou (código $($p.ExitCode)). Confira a internet e rode de novo." }
        Escrever 'WebView2: instalado.' 'Green'
    }

    ExcecaoDefender

    # .msi: pula só se ESTE .msi (mesmo ProductCode) já está instalado. Todo build do piloto sai
    # 0.1.0, então comparar versão não basta; o template do Tauri aceita atualizar na mesma versão.
    $codigo = PropriedadeDoMsi $msi 'ProductCode'
    $nova = [version](PropriedadeDoMsi $msi 'ProductVersion')
    $instalados = ProdutosInstalados
    $atual = $null
    foreach ($c in $instalados) { $v = VersaoInstalada $c; if ($v) { $atual = $v } }

    if ($instalados -contains $codigo) {
        Escrever "Este .msi ($nova) já está instalado: nada a fazer."
    } elseif ($atual -and $atual -gt $nova) {
        Escrever "Já há uma versão mais nova instalada ($atual > $nova). Não vou voltar versão." 'Yellow'
    } else {
        PararApp
        Escrever "Instalando $NomeApp $nova$(if ($instalados) { " (substituindo o build instalado $atual)" })..."
        $logMsi = Join-Path $PastaLog "msi-$(Get-Date -Format yyyyMMdd-HHmmss).log"
        try {
            $p = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList @(
                '/i', "`"$msi`"", '/qn', '/norestart', '/l*v', "`"$logMsi`"")
            switch ($p.ExitCode) {
                0       { Escrever 'Instalado.' 'Green' }
                3010    { Escrever 'Instalado (o Windows pede reinício, mas não é obrigatório agora).' 'Green' }
                1618    { throw 'Outra instalação do Windows está em andamento. Espere terminar e rode de novo.' }
                default { throw "msiexec falhou com código $($p.ExitCode). Log: $logMsi" }
            }
        } catch {
            # Falhou depois de o PararApp ter parado o serviço: não deixar a máquina sem ele.
            if (InfoServico) { Start-Service -Name $Servico -ErrorAction SilentlyContinue }
            throw
        }
    }
    if (-not (Test-Path $Exe)) { throw "Executável não encontrado em $Exe depois da instalação." }
    if (MsiTemServico $msi) { ConferirServico }

}

# --- Etapa da máquina: desinstalar (administrador) -------------------------------------------
function EtapaRemocao {
    Escrever '== Desinstalação (máquina) ==' 'Cyan'
    PararApp

    $instalados = ProdutosInstalados
    if (-not $instalados) {
        Escrever "$NomeApp não está instalado."
    }
    foreach ($c in $instalados) {
        $logMsi = Join-Path $PastaLog "msi-remover-$(Get-Date -Format yyyyMMdd-HHmmss).log"
        $p = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList @(
            '/x', $c, '/qn', '/norestart', '/l*v', "`"$logMsi`"")
        switch ($p.ExitCode) {
            0       { Escrever "Removido ($c)." 'Green' }
            3010    { Escrever "Removido ($c); o Windows pede reinício." 'Green' }
            1605    { Escrever "Já não estava instalado ($c)." }
            1618    { throw 'Outra instalação do Windows está em andamento. Espere terminar e rode de novo.' }
            default { throw "msiexec /x falhou com código $($p.ExitCode). Log: $logMsi" }
        }
    }

    RemoverServicoOrfao
    PararApp   # cápsula que tenha sobrado em outra sessão

    try {
        if (@((Get-MpPreference).ExclusionPath) -contains $PastaApp) {
            Remove-MpPreference -ExclusionPath $PastaApp
            Escrever 'Defender: exceção removida.' 'Green'
        }
    } catch {
        Escrever "Defender: não deu para remover a exceção ($($_.Exception.Message))." 'Yellow'
    }
    # WebView2 fica: é componente do Windows, outros apps usam.

    if ($RemoverToken) { RemoverTokenDaMaquina }
    else { Escrever 'Token preservado no cofre da máquina (reinstalar não gasta outro código). Para apagar: -RemoverToken.' }
}

# --- Enrolamento (ELEVADO: o token vai para o cofre da máquina) -------------------------------
# Antes do cofre, rodava fora da elevação e gravava no Credential Manager do usuário; agora o
# --enrolar sem elevação falha (não consegue fechar a ACL da pasta). As 3 regras do rate limit valem
# igual.
# Saídas do enrolamento (iguais às do --enrolar, e as do script com -Codigo):
#   0 enrolado (ou já estava) · 2 código recusado · 3 limite de tentativas · 4 sem conexão · 5 outro
$SaidaCodigoRecusado = 2

# Uma chamada ao --enrolar com um código. O 429 é esperado dentro do executável (até 3x, teto 300 s).
# Nunca escreve o código em log nem na tela.
function TentarCodigo([string]$codigo) {
    $saida = Join-Path $env:TEMP "apice-enrolar-$PID.txt"
    $erros = Join-Path $env:TEMP "apice-enrolar-$PID-err.txt"
    try {
        $p = Start-Process -FilePath $Exe -ArgumentList '--enrolar', $codigo -Wait -PassThru -NoNewWindow `
            -RedirectStandardOutput $saida -RedirectStandardError $erros
        $texto = Get-Content $saida -Raw -ErrorAction SilentlyContinue
        if ($texto) { $texto = $texto.Trim() }
    } finally {
        Remove-Item $saida, $erros -Force -ErrorAction SilentlyContinue
    }
    switch ($p.ExitCode) {
        0 {
            Escrever "Enrolado: $texto" 'Green'
            # O serviço lê o cofre ao subir: reinicia para ele pegar o token novo agora.
            if (InfoServico) {
                Restart-Service -Name $Servico -Force -ErrorAction SilentlyContinue
                Escrever 'Serviço reiniciado para ler o token novo.'
            }
            return 0
        }
        2 { Escrever 'Código recusado pelo Hub (inválido, expirado ou já usado). Gere um código NOVO em /admin/dispositivos; este não será tentado de novo.' 'Red'; return 2 }
        3 { Escrever 'Limite de tentativas do escritório atingido (muitos PCs ao mesmo tempo). Espere uns 5 minutos e rode de novo com o MESMO código: ele ainda vale.' 'Red'; return 3 }
        4 { Escrever "Sem conexão com o Hub ($texto). Confira a rede e rode de novo com o mesmo código." 'Red'; return 4 }
        default { Escrever "Erro no enrolamento (saída $($p.ExitCode)): $texto. Avise o responsável pelo piloto." 'Red'; return 5 }
    }
}

# Devolve a saída (0 = enrolado). Com -Codigo: UMA tentativa, sem perguntar. Sem -Codigo: pergunta
# na janela de administrador, e código recusado nunca é reenviado, nem se digitarem de novo.
function Enrolar {
    Escrever '== Enrolamento ==' 'Cyan'
    if (-not (EhAdmin)) { throw 'O enrolamento precisa de administrador (cofre da máquina).' }

    if ((JaEnrolado) -and -not $Reenrolar) {
        Escrever 'Esta máquina já está enrolada (cofre da máquina). Para trocar o token: -Reenrolar' 'Green'
        return 0
    }

    if ($Codigo) {
        Escrever 'Enrolamento: uma tentativa com o código informado.'
        return (TentarCodigo $Codigo)
    }

    $recusados = @{}
    while ($true) {
        $digitado = ([string](Read-Host 'Código de enrolamento (gerado no Hub; Enter vazio para sair)')).Trim()
        if (-not $digitado) { Escrever 'Enrolamento não feito. Rode o script de novo quando tiver o código.' 'Yellow'; return 5 }
        if ($digitado -notmatch '^[A-Za-z0-9-]{4,64}$') { Escrever 'Esse código não tem o formato esperado (letras, números e hífen).' 'Red'; continue }
        if ($recusados.ContainsKey($digitado.ToUpper())) {
            Escrever 'Esse código já foi recusado pelo Hub. Peça um código NOVO ao admin.' 'Red'
            continue
        }
        $r = TentarCodigo $digitado
        if ($r -eq 0) { return 0 }
        if ($r -eq $SaidaCodigoRecusado) { $recusados[$digitado.ToUpper()] = $true; continue }
        if ($r -in 3, 4) { continue }   # quem instala decide quando tentar de novo (mesmo código vale)
        return $r
    }
}

function RemoverTokenDaMaquina {
    if (-not (Test-Path -LiteralPath $Cofre)) { Escrever 'Token: o cofre da máquina já estava vazio.'; return }
    Remove-Item -LiteralPath $Cofre -Force
    Remove-Item -LiteralPath "$Cofre.tmp" -Force -ErrorAction SilentlyContinue
    Escrever 'Token removido do cofre da máquina. Para usar de novo, será preciso um código novo.' 'Green'
}

# LEGADO: token antigo no Credential Manager do usuário (antes do cofre).
function RemoverTokenDoUsuario {
    $alvos = AlvosDoToken
    if (-not $alvos) { return }
    foreach ($a in $alvos) {
        cmdkey "/delete:$a" | Out-Null
        if ($LASTEXITCODE -ne 0) { cmdkey "/delete:LegacyGeneric:target=$a" | Out-Null }
    }
    if (AlvosDoToken) { Escrever 'Token antigo: não consegui remover do Gerenciador de Credenciais (Credenciais genéricas).' 'Yellow' }
    else { Escrever 'Token antigo do Gerenciador de Credenciais removido.' }
}

# --- Início automático (HKCU\Run do usuário da sessão) -----------------------------------------
# O app usa o tauri-plugin-autostart 2.7 (auto-launch 0.6): valor em HKCU\...\Run com o NOME do app
# (package_info().name = productName "Ápice Gestor") e o dado "<exe> --sessao". Se a pessoa
# desativa em Gerenciador de Tarefas → Inicializar, o Windows grava o mesmo nome em
# ...\Explorer\StartupApproved\Run com os 8 últimos bytes ≠ 0 (o mesmo critério do auto-launch).
# Decisão do José Luiz (09/10): o instalador RESPEITA essa desativação e não religa.
$ChaveRun      = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Run'
$ChaveAprovado = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run'
$ArgsSessao    = '--sessao'

function NomesNoRun {
    # Valores do Run que apontam para o apice-gestor.exe (o nome pode não ser o esperado).
    $nomes = @()
    $chave = Get-Item -Path $ChaveRun -ErrorAction SilentlyContinue
    if ($chave) {
        foreach ($n in $chave.GetValueNames()) {
            if ([string]$chave.GetValue($n) -like '*apice-gestor.exe*') { $nomes += $n }
        }
    }
    return @($nomes)
}

function DesativadoPeloUsuario([string]$nome) {
    $chave = Get-Item -Path $ChaveAprovado -ErrorAction SilentlyContinue
    if (-not $chave) { return $false }
    $bytes = $chave.GetValue($nome)
    if (-not ($bytes -is [byte[]]) -or $bytes.Length -lt 8) { return $false }
    return [bool]($bytes[-8..-1] | Where-Object { $_ -ne 0 })
}

function RegistrarAutostartDoUsuario {
    $existentes = NomesNoRun
    $nome = if ($existentes) { $existentes[0] } else { $NomeApp }
    if (DesativadoPeloUsuario $nome) {
        Escrever "Início automático: desativado por quem usa o PC (Gerenciador de Tarefas). Respeitado, não religo." 'Yellow'
        return
    }
    if ($existentes) {
        Escrever 'Início automático: já registrado.'
        return
    }
    if (-not (Test-Path $ChaveRun)) { New-Item -Path $ChaveRun -Force | Out-Null }
    New-ItemProperty -Path $ChaveRun -Name $nome -PropertyType String -Value "`"$Exe`" $ArgsSessao" -Force | Out-Null
    Escrever 'Início automático: registrado (o Gestor abre sozinho ao entrar no Windows).' 'Green'
}

# Remove o valor do Run (o msiexec /x não sabe dele). A marca de "desativado" em StartupApproved
# fica: é escolha de quem usa o PC e tem que valer numa reinstalação.
function RemoverAutostartDoUsuario {
    foreach ($n in (NomesNoRun)) {
        Remove-ItemProperty -Path $ChaveRun -Name $n
        Escrever "Início automático removido ($n)." 'Green'
    }
}

function SessaoEhDesteUsuario([switch]$Calado) {
    # Se este PowerShell está elevado como OUTRA conta (a da TI), o token iria para a conta errada.
    if (-not (EhAdmin)) { return $true }
    $sessao = (Get-CimInstance Win32_ComputerSystem).UserName      # DOMINIO\usuario da sessão aberta
    $eu = "$env:USERDOMAIN\$env:USERNAME"
    if ($sessao -and ($sessao -ne $eu)) {
        if ($Calado) { return $false }
        Write-Host "Este PowerShell roda como $eu, mas a sessão aberta é de $sessao." -ForegroundColor Red
        Write-Host 'O token é da conta da sessão. Rode de novo como o colaborador, sem "Executar como administrador".' -ForegroundColor Red
        return $false
    }
    return $true
}

# --- Fluxo -----------------------------------------------------------------------------------
# O instalar.cmd entrega o código por APICE_CODIGO (nunca em linha de comando nem em histórico):
# lê e apaga do ambiente na hora.
if (-not $Codigo -and $env:APICE_CODIGO) {
    $Codigo = $env:APICE_CODIGO
    Remove-Item Env:APICE_CODIGO -ErrorAction SilentlyContinue
}
if ($Codigo) {
    $Codigo = $Codigo.Trim()
    # Formato antes de qualquer coisa (e antes do UAC): o código vai como argumento do --enrolar.
    if ($Codigo -notmatch '^[A-Za-z0-9-]{4,64}$') { Write-Host 'Código com formato inválido (só letras, números e hífen).' -ForegroundColor Red; exit 2 }
    if ($SemEnrolar) { Write-Host '-Codigo e -SemEnrolar juntos não fazem sentido.' -ForegroundColor Red; exit 5 }
}
# Instâncias elevadas: só a parte da máquina.
if ($Etapa -eq 'Maquina') {
    try { EtapaMaquina (Resolve-Path $Msi).Path } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
    if ($SemEnrolar) { exit 0 }
    $r = Enrolar
    # Sem -Codigo a janela de administrador é interativa: segura a mensagem antes de fechar.
    if ($r -ne 0 -and -not $Codigo) { Read-Host 'Enrolamento não concluído. Enter para fechar' | Out-Null }
    exit $r
}
if ($Etapa -eq 'Remocao') {
    try { EtapaRemocao; exit 0 } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
}

if ($Desinstalar) {
    # Por usuário só sobrou o LEGADO (Run e o token antigo do Credential Manager): limpa se este
    # PowerShell é o da sessão. O token de hoje (cofre) é da máquina: vai na etapa elevada.
    if (SessaoEhDesteUsuario -Calado) {
        RemoverAutostartDoUsuario
        if ($RemoverToken) { RemoverTokenDoUsuario }
    }
    if (EhAdmin) {
        try { EtapaRemocao } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
    } else {
        Write-Host 'Pedindo permissão de administrador para desinstalar...' -ForegroundColor Cyan
        $extras = @(); if ($RemoverToken) { $extras += '-RemoverToken' }
        if ((RelancarElevado 'Remocao' $extras) -ne 0) { Write-Host "A desinstalação falhou (veja $PastaLog\instalar.log)." -ForegroundColor Red; exit 1 }
    }
    exit 0
}

if (-not $Msi) {
    $candidatos = @(Get-ChildItem -Path $PSScriptRoot -Filter 'apice-gestor_*_x64.msi' -ErrorAction SilentlyContinue)
    if ($candidatos.Count -ne 1) { Write-Host "Coloque exatamente um apice-gestor_*_x64.msi ao lado do script (achei $($candidatos.Count)) ou use -Msi." -ForegroundColor Red; exit 1 }
    $Msi = $candidatos[0].FullName
}
$Msi = (Resolve-Path $Msi).Path

# Confere o hash, se o SHA256SUMS.txt do CI estiver junto.
$somas = Join-Path (Split-Path $Msi) 'SHA256SUMS.txt'
if (Test-Path $somas) {
    $linha = Select-String -Path $somas -Pattern ([regex]::Escape((Split-Path $Msi -Leaf))) | Select-Object -First 1
    if ($linha) {
        $h = (Get-FileHash $Msi -Algorithm SHA256).Hash.ToLower()
        if (-not $linha.Line.StartsWith($h)) { Write-Host 'SHA-256 do .msi NÃO confere com SHA256SUMS.txt. Não instale.' -ForegroundColor Red; exit 1 }
        Write-Host "SHA-256 confere: $h" -ForegroundColor Green
    }
}
# Baixado do GitHub, o .msi vem com a marca "veio da internet"; o hash já foi conferido.
Unblock-File -Path $Msi -ErrorAction SilentlyContinue

# Etapa da máquina: relança elevado (UAC). Quem autoriza pode ser o colaborador ou a TI.
# O enrolamento roda nessa mesma etapa elevada (cofre da máquina): a janela de administrador pede
# o código.
$saidaMaquina = 0
if (EhAdmin) {
    try { EtapaMaquina $Msi } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
    if (-not $SemEnrolar) { $saidaMaquina = Enrolar }
} else {
    Write-Host 'Pedindo permissão de administrador para instalar (o código de enrolamento é pedido na janela de administrador)...' -ForegroundColor Cyan
    $extras = @('-Msi', "`"$Msi`"")
    if ($SemEnrolar) { $extras += '-SemEnrolar' }
    if ($Reenrolar)  { $extras += '-Reenrolar' }
    # O código vai na linha de comando da instância elevada (só administradores leem a linha de
    # comando de um processo elevado); nunca em arquivo nem em log.
    if ($Codigo)     { $extras += @('-Codigo', $Codigo) }
    $saidaMaquina = RelancarElevado 'Maquina' $extras
    if ($saidaMaquina -eq 1) { Write-Host "A etapa da máquina falhou (veja $PastaLog\instalar.log)." -ForegroundColor Red; exit 1 }
    if ($saidaMaquina -ne 0) {
        $motivo = switch ($saidaMaquina) {
            2 { 'código recusado: gere um código NOVO em /admin/dispositivos' }
            3 { 'limite de tentativas: espere ~5 min e rode de novo com o mesmo código' }
            4 { 'sem conexão com o Hub: confira a rede e rode de novo com o mesmo código' }
            default { "erro no enrolamento (saída $saidaMaquina)" }
        }
        Write-Host "Instalado, mas NÃO enrolado: $motivo." -ForegroundColor Yellow
    }
}

# Início automático e token são da conta da sessão: nunca gravar na conta da TI.
if (InfoServico) {
    # Com o serviço, é ele que abre a cápsula em cada sessão: o Run vira legado e sai.
    if (SessaoEhDesteUsuario -Calado) { RemoverAutostartDoUsuario }
    Escrever 'Início automático: feito pelo serviço do Windows.'
} elseif (SessaoEhDesteUsuario -Calado) {
    RegistrarAutostartDoUsuario
} else {
    Escrever 'Início automático: NÃO registrado (este PowerShell não é o do colaborador). Rode o script de novo na conta dele.' 'Yellow'
}

if ($saidaMaquina -ne 0) { exit $saidaMaquina }
# Sem serviço (builds antigos), ninguém abre a cápsula: abre aqui, na sessão de quem rodou.
if (-not (InfoServico) -and -not (EhAdmin) -and -not (Get-Process -Name $Processo -ErrorAction SilentlyContinue)) {
    Start-Process -FilePath $Exe
}
Escrever 'Pronto.' 'Green'
exit 0
