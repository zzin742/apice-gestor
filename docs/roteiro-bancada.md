# Roteiro da bancada — PC do Vinny (Windows 10)

Piloto do Ápice Gestor em **um** PC. Leva uns 15 minutos. Qualquer passo que der diferente do
esperado: **pare, anote (seção 6) e não tente consertar**.

> **Antes de tudo:** só comece com o **José Luiz presente** (regra de 08/10). O app abre uma
> janela sempre no topo; ninguém instala nem abre o app sem ele.

## O que você recebe

- `apice-gestor_0.1.0_x64.msi` e `SHA256SUMS.txt`: artefato da **run manual do build-msi** da
  branch **`feat/servico`** que o Vigia indicou: **run [38393784525](https://github.com/zzin742/apice-gestor/actions/runs/38393784525)** (`6bfa888`),
  SHA-256 `88d735e35cc50d39322e16860109ceadeb883c5e1e51de1600ca68aa55829640`. Este `.msi` instala o **serviço do Windows** "Ápice Gestor" (conta SYSTEM,
  início automático), que abre a cápsula sozinho em cada logon. Fala com o **Hub de produção**
  (`https://www.apicehub.com.br/api/gestor`). **Não use .msi de outra run.**
- **Códigos de enrolamento**, gerados pelo José Luiz em `https://www.apicehub.com.br/admin/dispositivos`
  (24 h, uso único, aparecem uma vez só: anote em papel e não mande por WhatsApp). **Peça 3**: um
  para o enrolamento (passo 3) e dois de reserva para os testes negativos do cofre (passo 5A),
  caso o app os gaste.

## 0. Antes de começar (pré-requisitos)

- [ ] **José Luiz presente** (regra de 08/10).
- [ ] **O Hub está pronto para enrolar:** o Vigia confirmou com o Farol que as rotas
      `/api/gestor/*` estão em produção e que o Hub alcança o banco (hub_reader/DSN na Vercel).
      **Sem esse OK, a bancada para no passo 3.**
- [ ] **Duas contas no PC:** a do Vinny (administrador) **e uma conta de usuário COMUM** (padrão,
      sem administrador). Se não existir, crie uma conta local padrão só para o teste. Os passos
      5A e 5B (itens 6 e 9) só provam alguma coisa com ela.
- [ ] O PC do Vinny tinha o app do mock/Hub sem serviço, com o token no Gerenciador de
      Credenciais. Esse token **não vale mais** (o token agora fica no cofre da máquina): o passo 3
      enrola de novo.

## 1. O PC enxerga o Hub?

No navegador do PC do Vinny, abra:

    https://www.apicehub.com.br

- **Esperado:** o site do Ápice Hub abre.
- **Se não abrir:** pare aqui. É a internet ou um bloqueio da rede, não o app. Anote o que o
  navegador mostrou.

## 2. Conferir e instalar o .msi

Abra o **PowerShell** (normal, sem "Executar como administrador") na pasta dos dois arquivos:

```powershell
(Get-FileHash .\apice-gestor_0.1.0_x64.msi -Algorithm SHA256).Hash.ToLower()
Get-Content .\SHA256SUMS.txt
```

- **Esperado:** os dois códigos são iguais e batem com o SHA-256 que o Vigia passou.
- **Se forem diferentes:** não instale.

Antes de instalar, crie a **exceção no Defender** para a pasta do app. Abra um **segundo
PowerShell como administrador** (menu Iniciar → digite PowerShell → botão direito → *Executar como
administrador*) e cole:

```powershell
$pasta = Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor')
Add-MpPreference -ExclusionPath $pasta
(Get-MpPreference).ExclusionPath -contains $pasta
```

- **Esperado:** a última linha responde `True`. Rodar de novo não duplica nada.
- **Se der erro** ("0x800106ba" ou outro antivírus no lugar do Defender): anote e siga. Avise o
  Vigia qual antivírus o PC usa.

**Deixe esse PowerShell de administrador aberto:** os passos 3 e 5 usam ele.

Depois, dê **dois cliques no .msi**. O Windows vai avisar que o arquivo não é assinado ("O Windows
protegeu o computador" → **Mais informações → Executar assim mesmo**). É esperado: ainda não temos
certificado. Aceite o UAC.

- **Esperado:** instala sem erro, em `C:\Program Files\Ápice Gestor\`, e cria o **serviço
  "Ápice Gestor"** (`Get-Service ApiceGestor` no PowerShell de administrador: `Running`).
- **O PC já tinha um .msi anterior** (o do mock/do Hub sem serviço)? Pode instalar por cima: é uma
  atualização. Esperado: sem pedido de reinício.
- Se o PC não tiver o WebView2, o instalador baixa sozinho (precisa de internet).

## 3. Enrolar a máquina — PowerShell de ADMINISTRADOR, uma vez

O token agora fica no **cofre da máquina** (`%ProgramData%\ApiceGestor\device.token`, cifrado e
legível só por SYSTEM e Administradores), e quem o usa é o serviço. Por isso este passo roda **no
PowerShell de administrador**. Sem elevação ele **falha** (é o teste 5A-3).

**Antes, a parte 1 do teste do cofre** (prova que o enrolamento fecha uma pasta "plantada"): entre
na **conta de usuário comum**, abra um PowerShell normal e rode:

```powershell
New-Item -ItemType Directory -Force "$env:ProgramData\ApiceGestor" | Out-Null
Set-Content "$env:ProgramData\ApiceGestor\device.token.tmp" 'plantado pelo usuario comum'
```

Volte para a conta do Vinny e, **no PowerShell de administrador**:

```powershell
$exe   = Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor\apice-gestor.exe')
$saida = Join-Path $env:TEMP 'enrolar.txt'
$codigo = Read-Host 'Código de enrolamento'
$p = Start-Process -FilePath $exe -ArgumentList '--enrolar',$codigo -Wait -PassThru -NoNewWindow -RedirectStandardOutput $saida
"saída: $($p.ExitCode)"; Get-Content $saida; Remove-Item $saida
icacls "$env:ProgramData\ApiceGestor"
icacls "$env:ProgramData\ApiceGestor\device.token"
```

| Saída | Quer dizer | O que fazer |
|---|---|---|
| `0` + "Nome · Departamento" | enrolado | confira o `icacls` abaixo e siga |
| `2` | código inválido, expirado ou já usado | **não repita**. O José Luiz gera outro em `/admin/dispositivos` |
| `3` | limite de tentativas do escritório | espere 5 min; pode repetir **o mesmo** código uma vez |
| `4` | sem conexão com o Hub | volte ao passo 1 (e ao item "Hub pronto" do passo 0) |
| `5` / `64` / outro | erro do app | pare e anote o texto |

**Esperado no `icacls`** (pasta e arquivo): dono **NT AUTHORITY\SYSTEM** e acesso **só de
SYSTEM e Administradores** (nada de "Usuários", "Users" ou do nome do Vinny).

**Não rode o `--enrolar` duas vezes "para garantir".** O código é de uso único e o escritório
inteiro divide o limite de 10 tentativas por minuto.

*Nesta bancada, use o caminho manual acima, não o `instalar.ps1`:* o script já enrola no cofre da
máquina (feat/servico 49bd92a), mas a rodada 1 quer ver cada passo separado.

## 4. Conferir o painel (sem abrir nada: o serviço abre a cápsula)

Depois do enrolamento, a cápsula aparece sozinha em segundos (se não aparecer em 1 minuto: saia
da conta e entre de novo). **Não abra o app pelo menu Iniciar.**

- [ ] Aparece a **cápsula** no **canto inferior direito**, acima da barra de tarefas, sempre no
      topo, sem cobrir a área de trabalho.
- [ ] Ao abrir, o cartão mostra o **bom-dia** e recolhe sozinho em ~10 s.
- [ ] Ao passar o mouse ou clicar, abre o cartão com as abas **Hoje · Semana · Todas**.
- [ ] As tarefas são as do **José Luiz no Gestta** (o vínculo do dispositivo está com ele). Esperado
      em 08/10: **40 tarefas abertas, todas vencidas (TI), 0 hoje, 0 concluídas hoje**. O carimbo
      "atualizado às HH:MM" aparece.
- [ ] Em `https://www.apicehub.com.br/admin/dispositivos` a máquina aparece, com último contato e
      versão.
- [ ] Na aba **Todas**, a busca por cliente ou tarefa filtra.
- [ ] **Clicar numa tarefa abre ela no navegador** (Gestta).
- [ ] O ícone aparece na bandeja.

## 4b. Arrastar a cápsula — nesta ordem

- [ ] **1. Clique curto abre:** um clique rápido na cápsula abre o cartão (não arrasta).
- [ ] **2. Arrastar com o cartão fechado:** segure e arraste a cápsula recolhida para outro
      ponto. Ela acompanha o mouse e fica onde você soltar.
- [ ] **3. Arrastar logo depois do hover:** passe o mouse para o cartão abrir e, em seguida,
      comece a arrastar. **Esperado:** o cartão recolhe e só então a cápsula é arrastada.
- [ ] **4. Canto oposto:** solte a cápsula no **alto à esquerda** da tela e abra o cartão.
      **Esperado:** o cartão abre **para baixo e para a direita**, inteiro na tela.
- [ ] **5. Posição lembrada:** feche a cápsula pelo menu da bandeja. O serviço a reabre sozinho
      em segundos. **Esperado:** ela volta onde você deixou.
- [ ] **6. Voltar ao canto:** clique com o botão direito no ícone da bandeja → **Voltar ao canto**.
      **Esperado:** a cápsula volta ao canto inferior direito, acima da barra.

## 5. Serviço e cofre — o que só a bancada prova

O CI instala, atualiza e desinstala o serviço num Windows de verdade, mas **não tem sessão de
usuário**: tudo abaixo só se prova aqui. Comandos no **PowerShell de administrador**, menos onde
diz *usuário comum*. Anote a hora de cada item. O diário do serviço fica em
`%ProgramData%\ApiceGestor\servico.log` (sem token e sem segredo; só administrador lê).

### 5A. Cofre (ACL)

- [ ] **1. Pasta plantada:** já feito no passo 3. **Esperado:** `icacls` só com SYSTEM e
      Administradores, dono SYSTEM. Agora, na **conta de usuário comum**:
      `type "$env:ProgramData\ApiceGestor\device.token"` → **acesso negado**.
- [ ] **2. Temporário preso:** na **conta de usuário comum**, num PowerShell, segure o arquivo:
      `$f = [IO.File]::Open("$env:ProgramData\ApiceGestor\device.token.tmp", 'OpenOrCreate', 'ReadWrite', 'None')`
      (se der acesso negado já aqui, é porque a pasta está fechada: anote "bloqueado na pasta" e
      siga). Com ele aberto, `--enrolar` elevado com um **código de reserva**. **Esperado:** falha,
      **nada gravado** (o token do passo 3 continua valendo: o painel segue funcionando) e a
      mensagem pede administrador. Depois `$f.Close()`.
- [ ] **3. Sem elevação:** num PowerShell **normal** (sem administrador), `--enrolar` com o outro
      código de reserva. **Esperado:** falha (saída diferente de 0), **nada gravado**, painel segue
      funcionando.
- [ ] **4. Só o serviço lê:** `Restart-Service ApiceGestor`. **Esperado:** a máquina ganha um
      último contato novo em `/admin/dispositivos` e a cápsula volta com as tarefas; na **conta de
      usuário comum**, `type` no `device.token` continua dando **acesso negado**.

### 5B. Cápsula lançada pelo serviço (W6 1–9 + H5)

- [ ] **1. Ligar com usuário logado:** reinicie e entre no Windows **sem abrir nada**. **Esperado:**
      a cápsula aparece em **até 30 s** depois de a área de trabalho aparecer.
- [ ] **2. Cápsula morta volta:** `Stop-Process -Name apice-gestor -Force`. **Esperado:** volta sozinha.
- [ ] **3. Freio:** mate a cápsula **4 vezes seguidas** (repita o comando assim que ela voltar).
      **Esperado:** ela para de ser relançada por um tempo e o `servico.log` registra o freio.
- [ ] **4. Logoff/logon:** saia da conta e entre de novo. **Esperado:** uma cápsula nova e
      **uma só** (`Get-Process apice-gestor` mostra um processo para o usuário).
- [ ] **5. Bloquear/desbloquear:** Win+L e entre de novo. **Esperado:** nada quebra.
- [ ] **6. Quem roda o quê:**
      `Get-Process apice-gestor, apice-gestor-servico -IncludeUserName | Format-Table Name, Id, UserName`
      **Esperado:** `apice-gestor` (cápsula) com o **usuário** da sessão; `apice-gestor-servico`
      como **NT AUTHORITY\SYSTEM**. Repita com a **conta de usuário comum** logada.
- [ ] **7. Run vazio:** na conta do usuário,
      `Get-ItemProperty HKCU:\Software\Microsoft\Windows\CurrentVersion\Run` → **nenhum** valor do
      Ápice Gestor, depois de tudo.
- [ ] **8. Diário sem segredo:** `Get-Content "$env:ProgramData\ApiceGestor\servico.log" -Tail 50`
      mostra o enrolamento e **nenhum token**.
- [ ] **9. Usuário comum não lê o cofre:** igual ao 5A-1 (`type` → acesso negado).
- [ ] **10. Bandeja no logon (H5):** a cápsula pode subir antes do Explorer. Reinicie 2 ou 3 vezes
      e olhe a bandeja assim que a barra aparecer. **Esperado:** o **ícone do Ápice Gestor está na
      bandeja**. Anote se em alguma vez não estava e quanto tempo a barra levou.
- [ ] **11. Parar o serviço fecha a cápsula:** `Stop-Service ApiceGestor`. **Esperado:** a cápsula
      some e **não volta**. Depois `Start-Service ApiceGestor` → ela volta.

### 5C. Atualização com o serviço rodando

Quando o Vigia mandar o **próximo** `.msi`, instale por cima com a cápsula aberta (dois cliques ou
`msiexec /i`). **Esperado:** sem pedido de reinício, sem erro de arquivo em uso; o serviço volta
`Running` e a cápsula volta sozinha.

## 6. Desfazer (se o José Luiz pedir)

Configurações → Aplicativos → **Ápice Gestor** → Desinstalar. Isso remove o app **e o serviço**.

Ficam para trás (PowerShell **de administrador**):

```powershell
# Exceção do Defender
Remove-MpPreference -ExclusionPath (Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor'))
# Cofre e diário (o token da máquina): só se o José Luiz pedir para apagar o enrolamento
Remove-Item "$env:ProgramData\ApiceGestor" -Recurse -Force
```

O token antigo do mock no Gerenciador de Credenciais (**Credenciais do Windows → Credenciais
genéricas →** item com `br.com.apice.gestor`) pode ser removido à mão; o app novo não o usa.

## 7. Se falhar, anote

Mande para o Vigia, com foto da tela quando ajudar:

1. **Em qual passo/item** parou e o horário.
2. **Passo 1:** o que o navegador mostrou.
3. **Passo 2:** o resultado da exceção no Defender (`True` ou o erro), os dois SHA-256, a mensagem
   do instalador e o `Get-Service ApiceGestor`.
4. **Passo 3:** o número da **saída**, o texto impresso e a saída dos dois `icacls`. **Não** mande o
   código de enrolamento por WhatsApp/grupo.
5. **Passo 4:** o **estado do painel**: a cápsula apareceu sozinha? onde? Está âmbar ("dados de
   ontem"), "sem conexão", "não configurado", vazia ou com tarefas? Foto da cápsula e do cartão.
6. **Passo 4b:** qual item falhou, onde a cápsula estava e o que aconteceu.
7. **Passo 5:** o item, a hora, a saída do comando, e as **últimas linhas do `servico.log`** no
   mesmo minuto (`Get-Content "$env:ProgramData\ApiceGestor\servico.log" -Tail 30`).
8. Versão do Windows (`winver`) e se há outro antivírus além do Defender.

---

# RODADA 2 — desligar e bloquear (só quando o Vigia avisar)

> **Não faça antes do aviso do Vigia.** Depende do lado do Hub (Farol) estar em produção:
> `/admin/maquinas` com o desligar (contrato §8) e a lista de bloqueio (contrato §9). Vale para o
> **.msi da rodada 2**, que o Vigia indica (pode não ser o da rodada 1). **Não precisa de código de
> enrolamento novo:** a máquina já está enrolada desde a rodada 1. José Luiz presente, como sempre.
> Tudo no PowerShell **de administrador**, a não ser onde diz o contrário. O diário é o
> `%ProgramData%\ApiceGestor\servico.log`. Para acompanhar ao vivo durante os testes:
> `Get-Content "$env:ProgramData\ApiceGestor\servico.log" -Wait -Tail 5`.

## 8. Desligar pelo Hub (contrato §8)

**Como o admin desliga:** em `https://www.apicehub.com.br/admin/maquinas`, selecionar **a máquina
do Vinny** (a ordem é por máquina, não por pessoa: o PC do Vinny está vinculado ao José) e
**Desligar**. O histórico da ordem fica na mesma tela (pendente → reivindicada → executada /
falhou / vencida).

**O que o PC faz:** o serviço consulta as ordens a cada ~60 s, **reivindica** a ordem no Hub e só
então desliga, sem aviso na tela. Antes de desligar, grava o id da ordem em
`%ProgramData%\ApiceGestor\comandos-executados.json` (id gravado nunca roda de novo). No
`servico.log` aparecem `ordem cmd_… reivindicada` e `ordem cmd_… executada (desligando)`.

- [ ] **1. Desliga:** admin manda desligar. **Esperado:** o PC apaga em **até ~1 min, sem aviso**.
      Anote a hora em que o admin apertou e a hora em que apagou.
- [ ] **2. Não volta a desligar (o laço de junho):** ligue o PC de novo e entre no Windows (o
      serviço e a cápsula sobem sozinhos). **Esperado:** **não** desliga. Espere 3 min. **Repita 3
      vezes** (desligar pelo admin só na primeira; nas outras, só ligar e esperar). No `servico.log`,
      a ordem aparece **uma vez** como executada. No Hub, a ordem fica **executada** depois do
      primeiro heartbeat pós-boot.
- [ ] **3. Ordem antiga não vale (vence em 5 min):** desligue o PC **pelo menu Iniciar**. Com ele
      desligado, o admin manda desligar. Espere **10 min** e ligue. **Esperado:** **não** desliga; no
      Hub a ordem aparece **vencida**.
- [ ] **4. Inicialização Rápida (trava b, critério 7):** confira que está **ligada** (Painel de
      Controle → Opções de Energia → *Escolher a função dos botões de energia* → "Ligar
      inicialização rápida" marcado). Desligue **pelo menu Iniciar**, o admin manda a ordem, e
      ligue o PC **em até 2 min**. **Esperado:** **não** desliga. *(É o caso mais delicado: com a
      Inicialização Rápida, o serviço pode "acordar" em vez de reiniciar. Se desligar aqui, pare a
      rodada e avise o Vigia na hora.)*
- [ ] **5. Duas vezes = uma:** com o PC ligado, o admin manda desligar **duas vezes seguidas**.
      **Esperado:** desliga **uma vez**; ao ligar de novo, **não** desliga. No Hub, uma ordem só.
- [ ] **6. Sem rede = não desliga:** desconecte o cabo/Wi-Fi do PC. O admin manda desligar. Espere
      **6 min** (a ordem vence) e reconecte. **Esperado:** **não** desliga; no Hub, **vencida**.
      *(O caso exato de cair a rede entre a consulta e a reivindicação não dá para provocar à mão:
      fica para o teste de código do Chassi.)*
- [ ] **7. Sem conseguir gravar o id = não desliga (trava c, critério 8):** trave a escrita do
      serviço no arquivo e mande a ordem:
      ```powershell
      $f = "$env:ProgramData\ApiceGestor\comandos-executados.json"
      if (-not (Test-Path $f)) { Set-Content $f '[]' }
      icacls $f /deny "NT AUTHORITY\SYSTEM:(W)"
      ```
      Admin manda desligar. **Esperado:** **não** desliga; `servico.log` com `ordem cmd_… abortada:
      falha ao gravar o id`; no Hub, **falhou**. Depois, destrave:
      `icacls $f /remove:d "NT AUTHORITY\SYSTEM"`.

**Se falhar, anote:** item, hora do aperto no Hub, hora em que apagou (ou não), o estado da ordem no
Hub e as linhas de `ordem cmd_…` do `servico.log`.

## 9. Bloqueio de aplicativo (contrato §9)

**Como o admin bloqueia:** em `/admin/maquinas`, na **máquina do Vinny**, adicionar um item à lista
de bloqueio. Todo item precisa de **critério forte**: nome **e** caminho (`name_and_path`), nome
**e** editor, ou caminho exato. **Nome sozinho é recusado.** O serviço busca a lista a cada ~2 min
e fecha o app que casar, quando abre e também se já estiver aberto. No `servico.log`:
`bloqueio blk_… <nome> terminated` (ou `refused_protected`). **Nunca** aparece a lista do que a
pessoa abre, só o que casou.

**App de teste** (na conta do Vinny, PowerShell normal):

```powershell
Copy-Item C:\Windows\System32\notepad.exe "$env:USERPROFILE\Desktop\bloco-notas-copia.exe"
"$env:USERPROFILE\Desktop\bloco-notas-copia.exe"   # anote este caminho completo para o admin
```

Abra a cópia uma vez para ver que funciona. Se não abrir, use outro `.exe` portátil qualquer e
anote qual.

- [ ] **1. Bloqueia:** admin adiciona `bloco-notas-copia.exe` com **`name_and_path`** e o caminho
      anotado. Espere até 2 min e abra a cópia. **Esperado:** o app **fecha sozinho em segundos**;
      reabrir → fecha de novo. `servico.log` com `terminated`; no Hub, o histórico da máquina mostra
      o bloqueio.
- [ ] **2. Já aberto também fecha:** com o item ainda na lista, peça ao admin para tirá-lo, abra a
      cópia, e peça para pôr de volta. **Esperado:** em até 2 min a cópia aberta fecha.
- [ ] **3. Desbloqueia:** admin **tira** o item. Espere até 2 min. **Esperado:** a cópia **abre e
      fica** aberta.
- [ ] **4. Protegido é recusado:** o admin tenta bloquear `explorer.exe`, o próprio Ápice Gestor
      (`apice-gestor.exe` / `apice-gestor-servico.exe`) e o antivírus (`MsMpEng.exe`). **Esperado:**
      o **Hub recusa** cada um (erro de alvo protegido). Na máquina nada fecha. *(Se, por algum
      caminho, um protegido chegar à lista, o serviço não o encerra e o `servico.log` mostra
      `refused_protected`.)*
- [ ] **5. Só nome é recusado:** o admin tenta um item só com o nome (ex.: `notepad.exe`, sem
      caminho nem editor). **Esperado:** o **Hub recusa**. Na máquina, o Bloco de Notas normal abre
      e fica.
- [ ] **6. Sessão 0 nunca é encerrada (trava E3):** com a cópia **bloqueada** de novo (item 1),
      rode a mesma cópia **como serviço do sistema** (Sessão 0), por uma tarefa agendada:
      ```powershell
      $exe = "$env:USERPROFILE\Desktop\bloco-notas-copia.exe"
      schtasks /create /tn ApiceTesteE3 /tr "`"$exe`"" /sc once /st 23:59 /ru SYSTEM /f
      schtasks /run /tn ApiceTesteE3
      Start-Sleep 180
      Get-Process bloco-notas-copia -IncludeUserName | Format-Table Id, SessionId, UserName
      ```
      **Esperado:** depois de 3 min, o processo **da Sessão 0** (`SessionId` 0, `UserName` SYSTEM)
      **continua vivo**: o serviço só avalia processos de sessão de usuário. Abrir a cópia pela área
      de trabalho, ao mesmo tempo, continua **fechando** (item 1). Limpeza:
      `Stop-Process -Name bloco-notas-copia -Force; schtasks /delete /tn ApiceTesteE3 /f`.
- [ ] **7. Bloqueio desligado:** o admin põe o intervalo da lista em **0** para esta máquina
      (`blocklist_poll_interval_seconds = 0`). Espere até 5 min. **Esperado:** a cópia **abre e fica**,
      mesmo com o item na lista. Depois, o admin volta o intervalo para o normal.
- [ ] **8. Relatório é só bloqueio:** no histórico da máquina em `/admin/maquinas`, só aparecem os
      bloqueios (e recusas) dos itens da lista, **nunca** os outros programas que o Vinny abriu
      durante o teste (navegador, Bloco de Notas normal…).

**No fim:** o admin tira os itens de teste da lista; apague a cópia da Área de Trabalho.

**Se falhar, anote:** item, o que foi cadastrado no Hub (nome, critério, caminho), a resposta do
Hub, as linhas `bloqueio …` do `servico.log` e o `Get-Process … -IncludeUserName` do momento.

