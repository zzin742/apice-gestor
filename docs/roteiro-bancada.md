# Roteiro da bancada — PC do Vinny (Windows 10)

Piloto do Ápice Gestor em **um** PC. Leva uns 15 minutos. Qualquer passo que der diferente do
esperado: **pare, anote (seção 6) e não tente consertar**.

> **Antes de tudo:** só comece com o **José Luiz presente** (regra de 08/10). O app abre uma
> janela sempre no topo; ninguém instala nem abre o app sem ele.

## O que você recebe

- `apice-gestor_0.1.0_x64.msi` e `SHA256SUMS.txt`: artefato da **run manual do build-msi** que o
  Vigia indicou: **run [37792703891](https://github.com/zzin742/apice-gestor/actions/runs/37792703891)**,
  SHA-256 `3bbeafa2309aadb8654deef695d09db2cf5af7b465b89ffd303dbad86aecdc98`. **Não use .msi de outra run**: só a manual tem o
  endereço do mock embutido.
- O **código de enrolamento** do mock: `K7QM-4XPA`. Vale **uma vez**, só para o mock do Mac do
  José Luiz; não serve no Hub de verdade.
- O Mac do José Luiz ligado, na mesma rede, com o mock da Ponte rodando.

## 1. O PC enxerga o mock?

No navegador do PC do Vinny, abra:

    http://192.168.0.29:8787/__mock

- **Esperado:** a página do mock responde.
- **Se não abrir:** pare aqui. O problema é a rede ou o firewall do Mac, não o app. Anote o que o
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
$p = Start-Process -FilePath $exe -ArgumentList '--enrolar','K7QM-4XPA' -Wait -PassThru -NoNewWindow -RedirectStandardOutput $saida
"saída: $($p.ExitCode)"; Get-Content $saida; Remove-Item $saida
```

(O caminho do `.exe` está para o Chassi confirmar. Se `$exe` não existir, procure
`apice-gestor.exe` dentro de `C:\Program Files\` e anote onde estava.)

| Saída | Quer dizer | O que fazer |
|---|---|---|
| `0` + "Nome · Departamento" | enrolado | siga para o passo 4 |
| `2` | código inválido, expirado ou já usado | **não repita**. Peça um código novo ao José Luiz |
| `3` | limite de tentativas do escritório | espere 5 min; pode repetir **o mesmo** código uma vez |
| `4` | sem conexão com o mock | volte ao passo 1 |
| `5` / `64` / outro | erro do app | pare e anote |

**Não rode o comando duas vezes "para garantir".** O código é de uso único.

## 4. Abrir o app e conferir

Abra o **Ápice Gestor** pelo menu Iniciar.

- [ ] Aparece a **cápsula** no **canto inferior direito**, acima da barra de tarefas, sempre no
      topo, sem cobrir a área de trabalho.
- [ ] Ao abrir, o cartão mostra o **bom-dia** e recolhe sozinho em ~10 s.
- [ ] Ao passar o mouse ou clicar, abre o cartão com as abas **Hoje · Semana · Todas**.
- [ ] As tarefas são as do mock, e o carimbo "atualizado às HH:MM" aparece.
- [ ] Na aba **Todas**, a busca por cliente ou tarefa filtra.
- [ ] **Clicar numa tarefa abre ela no navegador** (Gestta).
- [ ] O ícone aparece na bandeja.

## 5. Desfazer (se o José Luiz pedir)

Configurações → Aplicativos → **Ápice Gestor** → Desinstalar. O token fica no Gerenciador de
Credenciais do Windows: **Credenciais do Windows → Credenciais genéricas →** item com
`br.com.apice.gestor` → Remover.

## 6. Se falhar, anote

Mande para o Vigia, com foto da tela quando ajudar:

1. **Em qual passo** parou (1 a 4) e o horário.
2. **Passo 1:** o que o navegador mostrou.
3. **Passo 2:** os dois SHA-256 e a mensagem do instalador (texto ou código de erro).
4. **Passo 3:** o número da **saída** e o texto impresso. **Não** mande o código de enrolamento
   por WhatsApp/grupo.
5. **Passo 4:** o **estado do painel**: a cápsula apareceu? onde? Está âmbar ("dados de ontem"),
   "sem conexão", "não configurado", vazia ou com tarefas? Foto da cápsula e do cartão aberto.
6. Versão do Windows (`winver`) e se há outro antivírus além do Defender.

---

*Depois do piloto, este roteiro vira o `scripts/instalar.ps1`, que faz os passos 2 e 3 sozinho
(com conferência de hash, exceção no Defender e o rate limit do enrolamento). Na bancada, os
passos são manuais de propósito: assim dá para ver onde falha.*
