<#
.SYNOPSIS
  Instala (ou desinstala) o Ápice Gestor num PC e enrola a máquina no Hub. Presencial, uma vez por PC.

.DESCRIPTION
  Rode COMO O COLABORADOR (a sessão dele aberta), sem "Executar como administrador":

      powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1
      powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Desinstalar

  Instalar tem três partes:
    1. Máquina (administrador; o script pede o UAC sozinho): WebView2 se faltar, o .msi em
       silêncio e a exceção no Defender. Pode ser autorizada com a senha da TI.
    2. Início automático (SEM elevação): registra o Gestor em HKCU\...\Run do colaborador, para
       ele abrir ao entrar no Windows sem ninguém abrir nada. Se a pessoa desativou no
       Gerenciador de Tarefas, respeita e não religa (decisão do José Luiz, 09/10).
    3. Enrolamento (SEM elevação, no usuário da sessão): pede o código gerado no Hub e chama
       "apice-gestor.exe --enrolar <código>". O token vai para o Credential Manager DO USUÁRIO que
       roda esta etapa; por isso ela nunca roda como a conta da TI.

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
  Saídas: 0 ok · 1 erro na etapa da máquina · 2 enrolamento não concluído.
#>
[CmdletBinding()]
param(
    [string]$Msi,
    [switch]$Reenrolar,
    [switch]$SemEnrolar,
    [switch]$Desinstalar,
    [switch]$RemoverToken,
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

# Alvos do Credential Manager do usuário atual que pertencem ao app (o keyring grava um
# "Generic credential" cujo alvo contém o serviço).
function AlvosDoToken {
    $alvos = @()
    foreach ($linha in (cmdkey /list 2>$null)) {
        if ($linha -match '(?:Target|Destino)\s*:\s*(.+)$' -and $Matches[1] -like "*$KeychainServico*") {
            $alvos += ($Matches[1].Trim() -replace '^LegacyGeneric:target=', '')
        }
    }
    return @($alvos)
}

function JaEnrolado { return [bool](AlvosDoToken) }

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
    # Fecha a cápsula em todas as sessões, mas nunca o processo do serviço: o MSI para o serviço do
    # jeito certo, e matar o processo faria o Windows reiniciá-lo (recuperação) no meio da instalação.
    $pidServico = $null
    $svc = InfoServico
    if ($svc) { $pidServico = $svc.ProcessId }
    $abertos = @(Get-Process -Name $Processo -ErrorAction SilentlyContinue | Where-Object { $_.Id -ne $pidServico })
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
        $p = Start-Process -FilePath 'msiexec.exe' -Wait -PassThru -ArgumentList @(
            '/i', "`"$msi`"", '/qn', '/norestart', '/l*v', "`"$logMsi`"")
        switch ($p.ExitCode) {
            0       { Escrever 'Instalado.' 'Green' }
            3010    { Escrever 'Instalado (o Windows pede reinício, mas não é obrigatório agora).' 'Green' }
            1618    { throw 'Outra instalação do Windows está em andamento. Espere terminar e rode de novo.' }
            default { throw "msiexec falhou com código $($p.ExitCode). Log: $logMsi" }
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
}

# --- Enrolamento (usuário da sessão, sem elevação) --------------------------------------------
function Enrolar {
    Escrever '== Enrolamento ==' 'Cyan'

    if ((JaEnrolado) -and -not $Reenrolar) {
        Escrever 'Esta máquina já está enrolada para este usuário. (Para trocar o token: -Reenrolar)' 'Green'
        return $true
    }

    $recusados = @{}   # códigos que o Hub recusou (saída 2): nunca reenviar
    $saida = Join-Path $env:TEMP "apice-enrolar-$PID.txt"
    $erros = Join-Path $env:TEMP "apice-enrolar-$PID-err.txt"
    try {
        while ($true) {
            $codigo = ([string](Read-Host 'Código de enrolamento (gerado no Hub; Enter vazio para sair)')).Trim()
            if (-not $codigo) { Escrever 'Enrolamento não feito. Rode o script de novo quando tiver o código.' 'Yellow'; return $false }
            if ($recusados.ContainsKey($codigo.ToUpper())) {
                Escrever 'Esse código já foi recusado pelo Hub. Peça um código NOVO ao admin.' 'Red'
                continue
            }

            # UMA chamada por código digitado. O 429 é esperado dentro do executável.
            $p = Start-Process -FilePath $Exe -ArgumentList '--enrolar', $codigo -Wait -PassThru -NoNewWindow `
                -RedirectStandardOutput $saida -RedirectStandardError $erros
            $texto = Get-Content $saida -Raw -ErrorAction SilentlyContinue
            if ($texto) { $texto = $texto.Trim() }

            switch ($p.ExitCode) {
                0 {
                    Escrever "Enrolado: $texto" 'Green'
                    return $true
                }
                2 {
                    # invalid_code / code_expired / code_used
                    Escrever 'Código inválido, expirado ou já usado. Peça um código NOVO ao admin do Hub.' 'Red'
                    $recusados[$codigo.ToUpper()] = $true
                }
                3 {
                    Escrever 'Limite de tentativas do escritório atingido (muitos PCs ao mesmo tempo).' 'Red'
                    Escrever 'Espere uns 5 minutos e digite o código de novo. O mesmo código ainda vale.' 'Yellow'
                }
                4 {
                    Escrever 'Sem conexão com o Hub. Confira a rede e digite o código de novo.' 'Red'
                }
                default {
                    Escrever "Erro no enrolamento (saída $($p.ExitCode)). Avise o responsável pelo piloto." 'Red'
                    return $false
                }
            }
        }
    } finally {
        Remove-Item $saida, $erros -Force -ErrorAction SilentlyContinue
        $codigo = $null
    }
}

function RemoverTokenDoUsuario {
    $alvos = AlvosDoToken
    if (-not $alvos) { Escrever 'Token: não havia token para este usuário.'; return }
    foreach ($a in $alvos) {
        cmdkey "/delete:$a" | Out-Null
        if ($LASTEXITCODE -ne 0) { cmdkey "/delete:LegacyGeneric:target=$a" | Out-Null }
    }
    if (JaEnrolado) { Escrever 'Token: não consegui remover. Apague no Gerenciador de Credenciais (Credenciais genéricas).' 'Yellow' }
    else { Escrever 'Token removido. Para usar de novo, será preciso um código novo.' 'Green' }
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
# Instâncias elevadas: só a parte da máquina.
if ($Etapa -eq 'Maquina') {
    try { EtapaMaquina (Resolve-Path $Msi).Path; exit 0 } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
}
if ($Etapa -eq 'Remocao') {
    try { EtapaRemocao; exit 0 } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
}

if ($Desinstalar) {
    # Autostart e token são da conta da sessão: tratados aqui, antes da elevação.
    if (-not (SessaoEhDesteUsuario)) { exit 2 }
    RemoverAutostartDoUsuario
    if ($RemoverToken) {
        RemoverTokenDoUsuario
    } else {
        Escrever 'Token preservado (reinstalar não gasta outro código). Para apagar: -RemoverToken.'
    }
    if (EhAdmin) {
        try { EtapaRemocao } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
    } else {
        Write-Host 'Pedindo permissão de administrador para desinstalar...' -ForegroundColor Cyan
        if ((RelancarElevado 'Remocao' @()) -ne 0) { Write-Host "A desinstalação falhou (veja $PastaLog\instalar.log)." -ForegroundColor Red; exit 1 }
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
if (EhAdmin) {
    try { EtapaMaquina $Msi } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
} else {
    Write-Host 'Pedindo permissão de administrador para instalar...' -ForegroundColor Cyan
    if ((RelancarElevado 'Maquina' @('-Msi', "`"$Msi`"")) -ne 0) { Write-Host "A etapa da máquina falhou (veja $PastaLog\instalar.log)." -ForegroundColor Red; exit 1 }
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

if ($SemEnrolar) { exit 0 }
if (-not (SessaoEhDesteUsuario)) { exit 2 }

if (Enrolar) {
    # Abre o app na sessão do colaborador (este processo é o dele).
    if (-not (Get-Process -Name $Processo -ErrorAction SilentlyContinue)) { Start-Process -FilePath $Exe }
    Escrever 'Pronto.' 'Green'
    exit 0
}
exit 2
