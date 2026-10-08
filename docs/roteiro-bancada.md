# Roteiro da bancada — PC do Vinny (Windows 10)

Piloto do Ápice Gestor em **um** PC. Leva uns 15 minutos. Qualquer passo que der diferente do
esperado: **pare, anote (seção 6) e não tente consertar**.

> **Antes de tudo:** só comece com o **José Luiz presente** (regra de 08/10). O app abre uma
> janela sempre no topo; ninguém instala nem abre o app sem ele.

## O que você recebe

- `apice-gestor_0.1.0_x64.msi` e `SHA256SUMS.txt`: artefato da **run manual do build-msi** que o
  Vigia indicou: **run [37833752635](https://github.com/zzin742/apice-gestor/actions/runs/37833752635)**
  (main `25c1654`), SHA-256 `3e81b7acb1332158374a783ce99895cf304353bfc48adc87a6aede3dc35a9827`.
  Esse `.msi` fala com o **Hub de produção**: `https://www.apicehub.com.br/api/gestor`.
  **Não use .msi de outra run:** as anteriores apontam para o mock do Mac do José Luiz.
- Um **código de enrolamento novo**, gerado pelo José Luiz em
  `https://www.apicehub.com.br/admin/dispositivos` (só admin; abre pela URL). Ele vale **24 h**, é
  de **uso único** e **aparece uma vez só** na tela: anote na hora, em papel, e não mande por
  WhatsApp nem por grupo.

> **O PC do Vinny já tem o app do mock, com o token do mock.** Esse token não vale no Hub de
> verdade, então é preciso **reenrolar**: desinstale o app (seção 5; pode manter o token, ele vai
> ser substituído), instale o `.msi` novo (passo 2) e **faça o passo 3 com o código novo**. O
> `--enrolar` grava o token novo por cima do antigo.

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

Depois, dê **dois cliques no .msi**. O Windows vai avisar que o arquivo não é assinado ("O Windows
protegeu o computador" → **Mais informações → Executar assim mesmo**). É esperado: ainda não temos
certificado. Aceite o UAC.

- **Esperado:** instala sem erro, em `C:\Program Files\Ápice Gestor\`.
- Se o PC não tiver o WebView2, o instalador baixa sozinho (precisa de internet).

## 3. Enrolar a máquina — UMA vez

No **mesmo PowerShell, sem elevação, logado como o Vinny**. O token fica guardado na conta de
quem roda este passo; se rodar como outra conta, o app não acha o token.

```powershell
$exe   = Join-Path $env:ProgramFiles ([char]0x00C1 + 'pice Gestor\apice-gestor.exe')
$saida = Join-Path $env:TEMP 'enrolar.txt'
$codigo = Read-Host 'Código de enrolamento'
$p = Start-Process -FilePath $exe -ArgumentList '--enrolar',$codigo -Wait -PassThru -NoNewWindow -RedirectStandardOutput $saida
"saída: $($p.ExitCode)"; Get-Content $saida; Remove-Item $saida
```

(O caminho do `.exe` está para o Chassi confirmar. Se `$exe` não existir, procure
`apice-gestor.exe` dentro de `C:\Program Files\` e anote onde estava.)

| Saída | Quer dizer | O que fazer |
|---|---|---|
| `0` + "Nome · Departamento" | enrolado | siga para o passo 4 |
| `2` | código inválido, expirado ou já usado | **não repita**. O José Luiz gera outro em `/admin/dispositivos` |
| `3` | limite de tentativas do escritório | espere 5 min; pode repetir **o mesmo** código uma vez |
| `4` | sem conexão com o Hub | volte ao passo 1 |
| `5` / `64` / outro | erro do app | pare e anote |

**Não rode o comando duas vezes "para garantir".** O código é de uso único, e o escritório inteiro
divide o limite de 10 tentativas por minuto.

*Alternativa com o script (só se o José Luiz liberar: o `instalar.ps1` ainda aguarda o "manda bala"):* `powershell -NoProfile -ExecutionPolicy Bypass -File .\instalar.ps1 -Reenrolar`
faz os passos 2 e 3 de uma vez, com o `.msi` e o `SHA256SUMS.txt` na mesma pasta do script.

## 4. Abrir o app e conferir

Abra o **Ápice Gestor** pelo menu Iniciar.

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
- [ ] **5. Posição lembrada:** feche o app (menu da bandeja ou Gerenciador de Tarefas) e abra de
      novo pelo menu Iniciar. **Esperado:** a cápsula volta onde você deixou.
- [ ] **6. Voltar ao canto:** clique com o botão direito no ícone da bandeja → **Voltar ao canto**.
      **Esperado:** a cápsula volta ao canto inferior direito, acima da barra.

## 5. Desfazer (se o José Luiz pedir)

Configurações → Aplicativos → **Ápice Gestor** → Desinstalar. O token fica no Gerenciador de
Credenciais do Windows: **Credenciais do Windows → Credenciais genéricas →** item com
`br.com.apice.gestor` → Remover.

## 6. Se falhar, anote

Mande para o Vigia, com foto da tela quando ajudar:

1. **Em qual passo** parou (1 a 4b) e o horário.
2. **Passo 1:** o que o navegador mostrou.
3. **Passo 2:** os dois SHA-256 e a mensagem do instalador (texto ou código de erro).
4. **Passo 3:** o número da **saída** e o texto impresso. **Não** mande o código de enrolamento
   por WhatsApp/grupo.
5. **Passo 4:** o **estado do painel**: a cápsula apareceu? onde? Está âmbar ("dados de ontem"),
   "sem conexão", "não configurado", vazia ou com tarefas? Foto da cápsula e do cartão aberto.
6. **Passo 4b:** qual item (1 a 6) falhou, onde a cápsula estava na tela e o que aconteceu
   (não mexeu, cartão não recolheu, abriu cortado, posição perdida...). Um vídeo do celular ajuda.
7. Versão do Windows (`winver`) e se há outro antivírus além do Defender.

---

*Depois do piloto, este roteiro vira o `scripts/instalar.ps1`, que faz os passos 2 e 3 sozinho
(com conferência de hash, exceção no Defender e o rate limit do enrolamento). Na bancada, os
passos são manuais de propósito: assim dá para ver onde falha.*
