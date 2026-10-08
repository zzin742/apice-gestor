<#
.SYNOPSIS
  Instala o Ápice Gestor num PC e enrola a máquina no Hub. Uma vez por PC, presencial.

.DESCRIPTION
  Rode COMO O COLABORADOR (a sessão dele aberta), sem "Executar como administrador":

      powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1

  O script tem duas etapas:
    1. Máquina (precisa de administrador; o script pede o UAC sozinho): WebView2, .msi e exceção
       no Defender. Pode ser autorizada com a senha de TI: nada desta etapa é do usuário.
    2. Enrolamento (roda SEM elevação, no usuário do colaborador): pede o código gerado no Hub e
       chama "apice-gestor.exe --enrolar <código>". O token vai para o Credential Manager DO
       USUÁRIO que roda esta etapa, por isso ela nunca roda como a conta de TI.

  Idempotente: rodar de novo não reinstala a mesma versão, não duplica a exceção e não enrola de
  novo uma máquina que já tem token (use -Reenrolar para trocar o token).

  Rate limit do POST /enroll (10/min por IP, e os 95 PCs saem pelo mesmo IP):
    - o executável já espera retry_after_seconds no 429 (até 3x, teto 300 s);
    - código inválido/expirado/usado: o script NUNCA repete; pede um código novo ao admin;
    - no máximo UMA tentativa automática por código; repetir é sempre decisão de quem instala.

  Não precisa de domínio nem de Microsoft 365. Não grava o código nem o token em log.

.PARAMETER Msi
  Caminho do .msi. Padrão: o apice-gestor_*_x64.msi da pasta do script. Se houver um
  SHA256SUMS.txt ao lado, o hash é conferido antes de instalar.

.PARAMETER Reenrolar
  Enrola de novo mesmo se a máquina já tiver token (ex.: dispositivo revogado no Hub).

.PARAMETER SemEnrolar
  Só a etapa da máquina.

.NOTES
  Arquivo salvo em UTF-8 COM BOM: o Windows PowerShell 5.1 lê sem BOM como ANSI e estraga os
  acentos. Saídas: 0 ok · 1 erro na etapa da máquina · 2 enrolamento não concluído.
#>
[CmdletBinding()]
param(
    [string]$Msi,
    [switch]$Reenrolar,
    [switch]$SemEnrolar,
    # Interno: usado quando o script se relança elevado para a etapa da máquina.
    [ValidateSet('Tudo', 'Maquina')]
    [string]$Etapa = 'Tudo'
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version 2

# --- Constantes ------------------------------------------------------------------------------
# Iguais às do src-tauri (tauri.conf.json e api.rs). Mudou lá, muda aqui.
$UpgradeCode    = '{97800A68-CABD-52E3-886A-6D8831965CFF}'
$PastaApp       = Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor')   # "Ápice Gestor"
$Exe            = Join-Path $PastaApp 'apice-gestor.exe'
$KeychainServico = 'br.com.apice.gestor'                                        # alvo no Credential Manager
$WebView2Id     = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
$WebView2Url    = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'           # Evergreen Bootstrapper
$PastaLog       = Join-Path $env:ProgramData 'ApiceGestor'

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

function VersaoDoMsi([string]$caminho) {
    $wi = New-Object -ComObject WindowsInstaller.Installer
    $db = $wi.GetType().InvokeMember('OpenDatabase', 'InvokeMethod', $null, $wi, @($caminho, 0))
    $view = $db.GetType().InvokeMember('OpenView', 'InvokeMethod', $null, $db,
        @("SELECT Value FROM Property WHERE Property='ProductVersion'"))
    $view.GetType().InvokeMember('Execute', 'InvokeMethod', $null, $view, $null) | Out-Null
    $rec = $view.GetType().InvokeMember('Fetch', 'InvokeMethod', $null, $view, $null)
    $v = $rec.GetType().InvokeMember('StringData', 'GetProperty', $null, $rec, 1)
    $view.GetType().InvokeMember('Close', 'InvokeMethod', $null, $view, $null) | Out-Null
    return [version]$v
}

function VersaoInstalada {
    try { return VersaoInstaladaCom } catch { Write-Verbose "COM do Windows Installer falhou; tentando o registro." }
    $raizes = 'HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\*',
              'HKLM:\SOFTWARE\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
    $app = Get-ItemProperty $raizes -ErrorAction SilentlyContinue |
        Where-Object { $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -eq ([char]0x00C1 + 'pice Gestor') } |
        Select-Object -First 1
    if ($app) { return [version]$app.DisplayVersion }
    return $null
}

function VersaoInstaladaCom {
    $wi = New-Object -ComObject WindowsInstaller.Installer
    $produtos = $wi.GetType().InvokeMember('RelatedProducts', 'GetProperty', $null, $wi, @($UpgradeCode))
    foreach ($p in $produtos) {
        $v = $wi.GetType().InvokeMember('ProductInfo', 'GetProperty', $null, $wi, @($p, 'VersionString'))
        if ($v) { return [version]$v }
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

function JaEnrolado {
    # keyring (Windows) grava um "Generic credential" cujo alvo contém o serviço.
    $lista = cmdkey /list 2>$null | Out-String
    return $lista -match [regex]::Escape($KeychainServico)
}

# --- Etapa 1: máquina (administrador) --------------------------------------------------------
function EtapaMaquina([string]$msi) {
    Escrever '== Etapa da máquina ==' 'Cyan'

    # 1a. WebView2: o .msi baixa o bootstrapper na instalação (decisão de 08/10). Aqui só garantimos.
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

    # 1b. .msi: só instala se não houver a mesma versão (ou mais nova).
    $nova = VersaoDoMsi $msi
    $atual = VersaoInstalada
    if ($atual -and $atual -ge $nova) {
        Escrever "Ápice Gestor $atual já instalado (o .msi é $nova): nada a fazer."
    } else {
        Escrever "Instalando Ápice Gestor $nova$(if ($atual) { " (atualizando de $atual)" })..."
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

    # 1c. Exceção no Defender (a pasta do app). Sem Defender ativo (outro antivírus): avisa e segue.
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

# --- Etapa 2: enrolamento (usuário do colaborador, sem elevação) -----------------------------
function Enrolar {
    Escrever '== Enrolamento ==' 'Cyan'

    if ((JaEnrolado) -and -not $Reenrolar) {
        Escrever 'Esta máquina já está enrolada para este usuário. (Para trocar o token: -Reenrolar)' 'Green'
        return $true
    }

    $recusados = @{}   # códigos que o Hub recusou (2): nunca reenviar, nem se digitarem de novo
    $saida = Join-Path $env:TEMP "apice-enrolar-$PID.txt"
    $erros = Join-Path $env:TEMP "apice-enrolar-$PID-err.txt"
    try {
        while ($true) {
            $codigo = (Read-Host 'Código de enrolamento (gerado no Hub; Enter vazio para sair)').Trim()
            if (-not $codigo) { Escrever 'Enrolamento não feito. Rode o script de novo quando tiver o código.' 'Yellow'; return $false }
            if ($recusados.ContainsKey($codigo.ToUpper())) {
                Escrever 'Esse código já foi recusado pelo Hub. Peça um código NOVO ao admin.' 'Red'
                continue
            }

            # UMA chamada por código. O 429 é esperado dentro do executável; aqui não há repetição automática.
            $p = Start-Process -FilePath $Exe -ArgumentList '--enrolar', $codigo -Wait -PassThru -NoNewWindow `
                -RedirectStandardOutput $saida -RedirectStandardError $erros
            $texto = (Get-Content $saida -Raw -ErrorAction SilentlyContinue)
            if ($texto) { $texto = $texto.Trim() }

            switch ($p.ExitCode) {
                0 {
                    Escrever "Enrolado: $texto" 'Green'
                    return $true
                }
                2 {
                    # invalid_code / code_expired / code_used: nunca repetir o mesmo código.
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

# --- Fluxo -----------------------------------------------------------------------------------
if (-not $Msi) {
    $candidatos = @(Get-ChildItem -Path $PSScriptRoot -Filter 'apice-gestor_*_x64.msi' -ErrorAction SilentlyContinue)
    if ($candidatos.Count -ne 1) { Write-Host "Coloque exatamente um apice-gestor_*_x64.msi ao lado do script (achei $($candidatos.Count)) ou use -Msi." -ForegroundColor Red; exit 1 }
    $Msi = $candidatos[0].FullName
}
$Msi = (Resolve-Path $Msi).Path

if ($Etapa -eq 'Maquina') {
    # Instância elevada: só a etapa da máquina.
    try { EtapaMaquina $Msi; exit 0 } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
}

# Confere o hash, se o SHA256SUMS.txt do CI estiver junto.
$somas = Join-Path (Split-Path $Msi) 'SHA256SUMS.txt'
if (Test-Path $somas) {
    $esperado = (Select-String -Path $somas -Pattern ([regex]::Escape((Split-Path $Msi -Leaf))) | Select-Object -First 1).Line
    if ($esperado) {
        $h = (Get-FileHash $Msi -Algorithm SHA256).Hash.ToLower()
        if (-not $esperado.StartsWith($h)) { Write-Host "SHA-256 do .msi NÃO confere com SHA256SUMS.txt. Não instale." -ForegroundColor Red; exit 1 }
        Write-Host "SHA-256 confere: $h" -ForegroundColor Green
    }
}

# Etapa da máquina: relança elevado (UAC). Quem autoriza pode ser o próprio colaborador ou a TI.
if (EhAdmin) {
    try { EtapaMaquina $Msi } catch { Escrever "ERRO: $($_.Exception.Message)" 'Red'; exit 1 }
} else {
    Write-Host 'Pedindo permissão de administrador para instalar...' -ForegroundColor Cyan
    $argsElevado = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$PSCommandPath`"", '-Etapa', 'Maquina', '-Msi', "`"$Msi`"")
    try {
        $p = Start-Process -FilePath 'powershell.exe' -ArgumentList $argsElevado -Verb RunAs -Wait -PassThru
    } catch {
        Write-Host 'Permissão de administrador negada. Nada foi instalado.' -ForegroundColor Red; exit 1
    }
    if ($p.ExitCode -ne 0) { Write-Host "A etapa da máquina falhou (veja $PastaLog\instalar.log)." -ForegroundColor Red; exit 1 }
}

if ($SemEnrolar) { exit 0 }

# Enrolamento: precisa ser o usuário da sessão (o dono do Credential Manager onde o app procura o token).
if (EhAdmin) {
    $sessao = (Get-CimInstance Win32_ComputerSystem).UserName      # DOMINIO\usuario da sessão aberta
    $eu = "$env:USERDOMAIN\$env:USERNAME"
    if ($sessao -and ($sessao -ne $eu)) {
        Write-Host "Este PowerShell roda como $eu, mas a sessão aberta é de $sessao." -ForegroundColor Red
        Write-Host 'O token ficaria na conta errada. Rode de novo como o colaborador, sem "Executar como administrador".' -ForegroundColor Red
        exit 2
    }
}

if (Enrolar) {
    # Abre o app para o colaborador (fora da elevação, já que este processo é o do usuário).
    if (-not (Get-Process -Name 'apice-gestor' -ErrorAction SilentlyContinue)) { Start-Process -FilePath $Exe }
    Escrever 'Pronto.' 'Green'
    exit 0
}
exit 2
