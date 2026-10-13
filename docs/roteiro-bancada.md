# Roteiro da bancada — PC do Vinny (Windows 10), UMA passada

Piloto do Ápice Gestor em **um** PC, numa passada só: **um clique** instala, ativa e enrola; depois
vêm os testes do serviço, da cápsula e do cofre, e os de **desligar** e **bloquear** pelo Hub.
Qualquer passo que der diferente do esperado: **pare, anote (seção 9) e não tente consertar**.

> **Antes de tudo:** só comece com o **José Luiz presente** (regra de 08/10). O app abre uma
> janela sempre no topo; ninguém instala nem abre o app sem ele.

## O que você recebe

O **pacote de um clique**, artefato da **run manual do build-msi** que o Vigia indicou:
**run [__RUN__](__URL__)** (`__COMMIT__`). Baixe o `.zip` da seção *Artifacts* da run e extraia
numa pasta (ex.: `C:\ApiceBancada`). Dentro:

| Arquivo | O que é |
|---|---|
| `apice-gestor_0.1.0_x64.msi` | o app + o **serviço do Windows** "Ápice Gestor" (SYSTEM, início automático), falando com o Hub de produção (`https://www.apicehub.com.br/api/gestor`) |
| `SHA256SUMS.txt` | o hash do `.msi` (o instalador confere sozinho). SHA-256 esperado: `__SHA__` |
| `instalar.cmd` | **o um clique** |
| `instalar.ps1` | o instalador que o `instalar.cmd` chama |

E **um código de enrolamento**, gerado pelo José Luiz em
`https://www.apicehub.com.br/admin/dispositivos` (24 h, uso único, aparece uma vez só: anote em
papel, não mande por WhatsApp). **Um basta:** os testes que devem falhar (5A) usam um código
qualquer, porque o app confere o cofre **antes** de chamar o Hub e não gasta código nenhum.

## 0. Antes de começar (pré-requisitos)

- [ ] **José Luiz presente.**
- [ ] **O Hub está pronto:** o Vigia confirmou com o Farol que estão em produção as rotas
      `/api/gestor/*` (fase 1, para enrolar e o painel) **e** o desligar/bloquear de
      `/admin/maquinas` (contrato §8/§9). Sem a fase 1, a bancada para no passo 2; sem §8/§9,
      faça até o passo 5 e pare.
- [ ] **Duas contas no PC:** a do Vinny (administrador) **e uma conta de usuário COMUM** (padrão,
      sem administrador). Se não existir, crie uma conta local padrão só para o teste. Os testes
      de cofre (5A) e o item 6 de 5B só provam alguma coisa com ela.
- [ ] **O PC já tem uma versão anterior do Ápice Gestor?** Comece do zero, no **PowerShell de
      administrador**, na pasta do pacote:
      `powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Desinstalar -RemoverToken`
      (tira o app, o serviço, a exceção do Defender e o token antigo). Se a pasta
      `%ProgramData%\ApiceGestor` sobrar, apague-a no mesmo PowerShell.

## 1. O PC enxerga o Hub?

No navegador do PC do Vinny, abra `https://www.apicehub.com.br`.

- **Esperado:** o site do Ápice Hub abre.
- **Se não abrir:** pare aqui. É a internet ou um bloqueio da rede, não o app.

## 2. UM CLIQUE: instala, ativa e enrola

**2.1. A pasta "plantada" (1ª parte do teste do cofre).** Entre na **conta de usuário comum**,
abra um PowerShell normal e rode:

```powershell
New-Item -ItemType Directory -Force "$env:ProgramData\ApiceGestor" | Out-Null
Set-Content "$env:ProgramData\ApiceGestor\device.token.tmp" 'plantado pelo usuario comum'
```

Saia dessa conta e volte para a do Vinny.

**2.2. O clique.** Na pasta do pacote, **dois cliques no `instalar.cmd`**:

1. Ele pede o **código de enrolamento**: digite e Enter.
2. O Windows pede **permissão de administrador** (UAC): **Sim**. (Pode aparecer o aviso de
   "editor desconhecido": o app não é assinado, decisão do José Luiz; siga.)
3. Uma janela de administrador faz tudo sozinha: confere o hash, cria a **exceção do Defender** (só
   a pasta do app), instala o `.msi`, sobe o **serviço**, e **enrola** com o código (uma tentativa
   só).
4. No fim, a janela do `instalar.cmd` mostra **PRONTO** ou **NÃO concluiu (saída N)**.

| Saída | Quer dizer | O que fazer |
|---|---|---|
| `0` — PRONTO | instalado, serviço rodando, máquina enrolada | siga |
| `1` | erro na instalação (máquina) | pare e anote; mande o `%ProgramData%\ApiceGestor\instalar.log` e o `msi-*.log` |
| `2` | código recusado (inválido, expirado ou já usado) | **não repita o mesmo**: o José Luiz gera outro; rode o `instalar.cmd` de novo (a instalação não se repete) |
| `3` | limite de tentativas do escritório | espere 5 min e rode de novo com o **mesmo** código |
| `4` | sem conexão com o Hub | volte ao passo 1 (e ao "Hub pronto" do passo 0); depois, mesmo código |
| `5` | outro erro no enrolamento | pare e anote o texto |

**2.3. Conferir** (PowerShell **de administrador**):

```powershell
Get-Service ApiceGestor                                   # Running
(Get-MpPreference).ExclusionPath                          # contém C:\Program Files\Ápice Gestor
icacls "$env:ProgramData\ApiceGestor"
icacls "$env:ProgramData\ApiceGestor\device.token"
```

- [ ] Serviço `Running`; exceção do Defender só da pasta do app (o antivírus continua ligado).
- [ ] `icacls` (pasta e arquivo): dono **NT AUTHORITY\SYSTEM** e acesso **só de SYSTEM e
      Administradores**, mesmo com a pasta plantada pelo usuário comum no 2.1.

## 3. Conferir o painel (sem abrir nada: o serviço abre a cápsula)

Depois do "PRONTO", a cápsula aparece sozinha em segundos (se não aparecer em 1 minuto: saia
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

## 4. Arrastar a cápsula — nesta ordem

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
diz *usuário comum*. O diário do serviço é o `%ProgramData%\ApiceGestor\servico.log` (sem token,
sem segredo; só administrador lê). Para acompanhar ao vivo:
`Get-Content "$env:ProgramData\ApiceGestor\servico.log" -Wait -Tail 5`.

Para os testes de enrolamento que **devem falhar**, use o comando direto do app com um código
qualquer (`TESTE-0000`); ele falha **antes** de chamar o Hub e **não gasta código**:

```powershell
$exe = Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor\apice-gestor.exe')
$p = Start-Process $exe -ArgumentList '--enrolar','TESTE-0000' -Wait -PassThru -NoNewWindow; "saída: $($p.ExitCode)"
```

### 5A. Cofre (ACL)

- [ ] **1. Pasta plantada:** já provado no 2.3 (`icacls` só SYSTEM + Administradores).
- [ ] **2. Usuário comum não lê:** na **conta de usuário comum**,
      `type "$env:ProgramData\ApiceGestor\device.token"` → **acesso negado**.
- [ ] **3. Temporário preso:** na **conta de usuário comum**, tente segurar o temporário:
      `$f = [IO.File]::Open("$env:ProgramData\ApiceGestor\device.token.tmp", 'OpenOrCreate', 'ReadWrite', 'None')`.
      Se der acesso negado já aqui, a pasta está fechada: anote "bloqueado na pasta" e siga. Se
      abrir, deixe aberto e rode o comando acima (`TESTE-0000`) **como administrador**.
      **Esperado:** saída diferente de 0, **nada gravado**, o painel continua funcionando. Depois
      `$f.Close()`.
- [ ] **4. Sem elevação:** num PowerShell **normal** (sem administrador), o mesmo comando com
      `TESTE-0000`. **Esperado:** falha (saída diferente de 0), **nada gravado**, painel funcionando.
- [ ] **5. Só o serviço lê:** `Restart-Service ApiceGestor`. **Esperado:** último contato novo em
      `/admin/dispositivos` e a cápsula volta com as tarefas; o usuário comum continua sem ler o
      `device.token`.

### 5B. Cápsula lançada pelo serviço (W6 1–9 + H5 + parar o serviço)

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
- [ ] **9. Usuário comum não lê o cofre:** igual ao 5A-2 (`type` → acesso negado).
- [ ] **10. Bandeja no logon (H5):** a cápsula pode subir antes do Explorer. Reinicie 2 ou 3 vezes
      e olhe a bandeja assim que a barra aparecer. **Esperado:** o **ícone do Ápice Gestor está na
      bandeja**. Anote se em alguma vez não estava e quanto tempo a barra levou.
- [ ] **11. Parar o serviço fecha a cápsula:** `Stop-Service ApiceGestor`. **Esperado:** a cápsula
      some e **não volta**. Depois `Start-Service ApiceGestor` → ela volta.

### 5C. Atualização com o serviço rodando

Quando o Vigia mandar o **próximo** pacote, rode o `instalar.cmd` novo por cima, com a cápsula
aberta (a máquina já está enrolada: o código não é pedido de novo; se pedir, deixe em branco e
anote). **Esperado:** sem pedido de reinício, sem erro de arquivo em uso; o serviço volta
`Running` e a cápsula volta sozinha.

---

> **Seções 6 e 7 dependem do §8/§9 do Hub em produção** (passo 0). Sem eles, pare aqui e anote até
> onde foi.

## 6. Desligar pelo Hub (contrato §8)

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
      bancada e avise o Vigia na hora.)*
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

## 7. Bloqueio de aplicativo (contrato §9)

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
      rode a mesma cópia **como o sistema** (Sessão 0), por uma tarefa agendada:
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

## 8. Desfazer (se o José Luiz pedir)

No **PowerShell de administrador**, na pasta do pacote:

```powershell
# Tira o app, o serviço e a exceção do Defender. O token fica no cofre (reinstalar não gasta código).
powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Desinstalar
# ...ou, para apagar também o enrolamento (token do cofre e o antigo do Gerenciador de Credenciais):
powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Desinstalar -RemoverToken
```

## 9. Se falhar, anote

Mande para o Vigia, com foto da tela quando ajudar:

1. **Em qual passo/item** parou e o horário.
2. **Passo 1:** o que o navegador mostrou.
3. **Passo 2:** a **saída** e a mensagem do `instalar.cmd`, o `Get-Service ApiceGestor`, os dois
   `icacls` e o `%ProgramData%\ApiceGestor\instalar.log`. **Não** mande o código de enrolamento.
4. **Passo 3:** o **estado do painel**: a cápsula apareceu sozinha? onde? Âmbar ("dados de ontem"),
   "sem conexão", "não configurado", vazia ou com tarefas? Foto da cápsula e do cartão.
5. **Passo 4:** qual item do arraste falhou, onde a cápsula estava e o que aconteceu.
6. **Passos 5 a 7:** o item, a hora, a saída do comando e as **últimas linhas do `servico.log`** no
   mesmo minuto (`Get-Content "$env:ProgramData\ApiceGestor\servico.log" -Tail 30`); no 6 e no 7,
   também o estado da ordem/da lista em `/admin/maquinas`.
7. Versão do Windows (`winver`) e se há outro antivírus além do Defender.
