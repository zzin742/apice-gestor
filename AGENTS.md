# Ápice Gestor — agente de desktop

Você desenvolve o **Ápice Gestor**: o aplicativo Windows que mostra ao colaborador as tarefas
do dia dele (fonte: Gestta, via API do Ápice Hub) e sustenta a escalação ao gestor por WhatsApp.
**Ainda não está em produção.** Nenhuma máquina de colaborador tem isso instalado.

- Pasta: `~/dev/apice-gestor` · repo `zzin742/apice-gestor` (privado) · branch **`main`**
- Vault: `~/dev/Jarvis/PROJETOS/Apice-Gestor/` — leia `Arquitetura.md` antes de qualquer coisa

## PASSO 0
1. `~/dev/Jarvis/PROJETOS/Apice-Gestor/Arquitetura.md` — o desenho inteiro e as decisões
2. `~/dev/Jarvis/PROJETOS/Apice-Gestor/Estado-*.md` — o mais recente
3. `src/painel.html` + `src/painel.css` + `src/painel.js` — a interface aprovada pelo José Luiz:
   cápsula vermelha (15/10/2026) e cartão vermelho e branco (16/10/2026), ambos aprovados em
   preview com screenshots antes de subir. **Veja no navegador:** `npm run mock` e abra
   `src/index.html`. `design/painel-do-dia.html` é o protótipo anterior (07/10), só histórico.
4. `~/dev/Jarvis/AGENTES/PROTOCOLO-GIT-MULTIAGENTE.md` e `AGENTES/status/`

## As três regras do projeto

1. **O agente nunca acessa banco de dados.** Nem o dos robôs, nem o do Hub. Ele fala **só com a
   API do Ápice Hub**, autenticado por um token por máquina. Se algo "precisa" de acesso direto,
   a resposta é uma rota nova na API, combinada com o Farol — não uma credencial no agente.
2. **A verdade do "feito" é o Gestta.** O agente é **só leitura**: não marca, não conclui, não
   escreve de volta. O ponto fica verde quando a tarefa está concluída no Gestta e o robô
   sincronizou. Mostre sempre o carimbo "atualizado às HH:MM" e a guarda de dado velho.
3. **Nada vai para máquina de colaborador sem o José Luiz dizer "manda bala".** Piloto é em
   **um** PC de bancada, nunca "só mais dois pra testar".

## Stack e build

- **Tauri v2** com a interface web do protótipo. Plugins oficiais: autostart, single-instance,
  notification, updater. Janela sem borda, transparente, sempre-no-topo; ícone na bandeja.
- **O `.msi` do Windows é gerado no GitHub Actions (`windows-latest`).** Não existe
  cruza-compilação do macOS. Desenvolva no Mac (`tauri dev` roda aqui), valide em Windows 11
  no Parallels e no PC de bancada.
- Instalador e executável **assinados** antes de qualquer máquina que não seja a bancada.
- Atualização automática pelo updater do Tauri; o manifesto é servido pelo Hub/Vercel ou por
  GitHub Releases — combinar com o José Luiz.
- Script de instalação (uma vez por PC): instala, exceção no Defender, enrolamento por código.

## Fronteiras com os outros Maestros

- **Farol @ APICEHUB** é dono de tudo que roda no Hub: rotas `/api/gestor/*`, telas de
  administração, crons de snapshot e escalação, tabelas `gestor_*`, role `hub_reader` no banco
  dos robôs. Você **propõe o contrato** (`docs/contrato-api.md`) e implementa o lado do agente.
  Não edite o repo do Hub.
- **Eco @ APICEZAP** é dono do canal de WhatsApp. A escalação sai pelo WhatsApice existente.
  Nunca proponha um Baileys novo.
- Dúvida de quem é o quê: pergunte ao Diretor ou ao José Luiz. Não chute.

## Antes de entregar
`cargo check`, `npm run build` da interface, e o `.msi` do CI abrindo no Parallels. Diga o que
muda e em quais máquinas.

## Regras do José Luiz ("Zezinho"), dono do produto

1. **Nada vai pra produção sem ele autorizar** — inclusive teste de viabilidade.
   Se dá pra provar no Mac, no Parallels ou na bancada, prove ali.
2. **Ele aprova dizendo "manda bala".** Antes disso, o que você tem é proposta.
3. **Mudança visual: mostrar antes de subir.** O protótipo foi iterado quatro vezes até ser
   aprovado; não "melhore" o visual sem mostrar.
4. **Segredo não entra em log nem em commit.** `.env` está no `.gitignore`. O **token de máquina**
   fica no **keychain do Windows** quando o agente roda na sessão do usuário; quando o agente roda como
   **serviço SYSTEM** (que não lê o keychain do usuário), fica num **arquivo cifrado da máquina**:
   DPAPI de **máquina** (só este PC descriptografa), permissão NTFS **só SYSTEM + Administradores**,
   **nunca texto puro, nunca em log, nunca em commit**. É o equivalente do keychain para um serviço.
5. **Errou? Diz direto, sem rodeio.**
6. Demanda ambígua: **pergunte, não chute.**

## Vault (`~/dev/Jarvis`) — obrigatório

Decisão, regra, credencial (ponteiro, nunca valor), erro resolvido, comando que funcionou,
pendência: tudo vai pro vault, em `PROJETOS/Apice-Gestor/`. **Commit e push fazem parte da tarefa.**

**Antes de escrever:** leia `~/dev/Jarvis/AGENTES/status/` e **declare sua sessão lá**
(formato no `status/README.md`). Apague quando terminar.
Escreva **só na sua própria subpasta**. Na raiz, **só edição aditiva**. **Nunca sobrescreva
arquivo que você não criou.**

Git: `fetch` → `add` (por nome, arquivo a arquivo) → `commit` → `pull --rebase` → `push`.
**Proibido:** `git add -A` · `--force` · `--amend` · `rebase -i` · `reset --hard`.

## 🔴 Segurança — vale em todos os projetos

Há chaves service-role do Supabase commitadas no vault e o José Luiz decidiu **não rotacionar
por ora** (detalhe em `PROJETOS/Maestri/Incidente-chaves-service-role-no-vault.md`). Para este
projeto isso significa: **não use service-role de projeto nenhum**; o agente não tem chave
nenhuma; o Hub lê o banco dos robôs com o role só-leitura `hub_reader`. A senha do `postgres`
do banco dos robôs **não existe para você** — se precisar de dado de lá, peça ao José Luiz um
extrato ou uma rota na API.

⚠️ **Não leia os `.jsonl` de `~/.claude/projects/`** — têm segredo em texto puro.
