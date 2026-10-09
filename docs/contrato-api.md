# Contrato da API — Ápice Gestor ↔ Ápice Hub

> **Versão 1 — proposta** (Ponte, 08/10/2026). Ainda não implementada no Hub; quem implementa é o
> `Farol @ APICEHUB`. Enquanto isso, o mock em `mock/` serve exatamente este contrato.
> Referência: `~/dev/Jarvis/PROJETOS/Apice-Gestor/Arquitetura.md` §3.2, §3.3, §4.

O agente fala **só** com estas quatro rotas. Não há credencial de banco no agente nem haverá:
qualquer dado novo que a interface precisar vira campo novo aqui, combinado com o Farol.

| Rota | Autenticação | Função |
|---|---|---|
| `POST /api/gestor/enroll` | código de enrolamento | troca o código por um token de dispositivo |
| `GET  /api/gestor/me/tasks` | token de dispositivo | tarefas da pessoa, já resolvidas em Hoje / Semana / Todas |
| `GET  /api/gestor/me/config` | token de dispositivo | flags e parâmetros que o agente obedece |
| `POST /api/gestor/heartbeat` | token de dispositivo | telemetria do agente |
| `GET  /api/gestor/me/commands` · `POST …/{id}/claim` · `POST …/{id}/result` | token de dispositivo | ordens ao dispositivo (desligar o PC) — **proposta, §8** |

---

## 1. Convenções

- **Base:** `https://<hub>/api/gestor`. No mock: `http://127.0.0.1:8787/api/gestor`.
- **Formato:** JSON UTF-8. Nomes de campo em `snake_case`, em inglês; textos para o usuário em português.
- **Datas:** `YYYY-MM-DD` são datas de calendário **no fuso `America/Sao_Paulo`**. Instantes são
  ISO 8601 **com offset** (`2026-10-08T13:02:00-03:00`). O agente não converte fuso: mostra o que vem.
- **"Hoje" é do servidor.** Toda resposta de tarefas traz `today`; o agente não usa o relógio da
  máquina para classificar tarefa (relógio de PC de escritório erra).
- **Campo desconhecido:** o agente **ignora** campo que não conhece. O Hub pode acrescentar campos
  sem subir a versão.

### 1.1 Versionamento

- Cada requisição do agente manda `X-Gestor-Api: 1` e `User-Agent: ApiceGestor/<versão> (<os>)`.
- Cada resposta traz `X-Gestor-Api: 1` e o campo `"api": 1` no corpo.
- **Mudança compatível** (campo novo, valor novo em lista aberta): mesma versão.
- **Mudança incompatível** (remover/renomear campo, mudar semântica): versão 2, e o Hub serve 1 e 2
  em paralelo até a última máquina atualizar (ver `min_agent_version` em §4).
- Se o agente pedir uma versão que o Hub não serve mais: `426 upgrade_required`.

### 1.2 Autenticação por token de dispositivo

- O token sai **uma vez**, na resposta do `enroll`. O Hub guarda só o **hash SHA-256** dele.
- O agente guarda o token no **Windows Credential Manager** (keychain), nunca em arquivo texto,
  nunca em log. No Mac de desenvolvimento, no Keychain.
- Envio: `Authorization: Bearer <token>`.
- Formato: `gdt_` + 43 caracteres base64url (256 bits). Opaco: o agente não interpreta.
- O token **não expira por tempo**; morre quando o admin **revoga** o dispositivo no Hub. Revogado →
  `401 token_revoked` e o agente volta para a tela de enrolamento.
- Um token = uma máquina = uma pessoa. Trocar a pessoa de uma máquina é reenrolar.

### 1.3 Cache — resposta autenticada é de uma pessoa só

Achado do Prisma (08/10/2026): com ETag igual entre pessoas e sem `Vary`, o cache HTTP do navegador
revalidou com `304` a resposta de outra pessoa, e o painel do Rafael mostrou os números do Caio.
Por isso, **obrigatório no Hub**:

- Todo ETag de rota autenticada (`me/tasks`, `me/config`) **inclui o id da pessoa**. ETag de outra
  pessoa nunca dá `304`.
- Toda resposta autenticada — `200`, `304` e erro — leva `Cache-Control: private, no-cache` e
  `Vary: Authorization`. Nenhum CDN/proxy (inclusive o da Vercel) guarda essas respostas.
- A resposta do `enroll` (que traz o token) leva `Cache-Control: no-store`.
- O cliente (`src/api/client.js`) guarda o ETag por instância e limpa ao trocar de token.

### 1.4 Erros

Todo erro tem o mesmo corpo:

```json
{ "api": 1, "error": { "code": "token_revoked", "message": "Este computador foi desconectado do Ápice Hub.", "retry_after_seconds": null } }
```

`message` é em português e **pode ser mostrada ao usuário**. O agente decide pelo `code`, nunca pelo texto.

| HTTP | `code` | Quando | O que o agente faz |
|---|---|---|---|
| 400 | `invalid_request` | corpo/parâmetro malformado | loga (sem token) e não repete |
| 400 | `invalid_code` | código de enrolamento não existe | pede o código de novo; **nunca repete sozinho** (§2) |
| 410 | `code_expired` | código passou da validade (sugestão: 24 h) | pede código novo ao admin |
| 409 | `code_used` | código já trocado por token | pede código novo ao admin |
| 401 | `unauthorized` | sem token / token desconhecido | volta ao enrolamento |
| 401 | `token_revoked` | admin revogou o dispositivo | apaga o token, volta ao enrolamento |
| 403 | `excluded_account` | a pessoa do dispositivo é conta-caixa (lista "contas que não são pessoa") | mostra a mensagem; não busca tarefas |
| 409 | `person_not_mapped` | dispositivo existe mas o de-para pessoa → nome no Gestta não está feito | mostra "aguardando configuração" e tenta de novo em `retry_after_seconds` |
| 409 | `command_gone` | ordem já reivindicada, vencida, cancelada ou de outro dispositivo (§8) | **não executa**; não repete |
| 426 | `upgrade_required` | versão do agente/API abaixo do mínimo | força o updater |
| 429 | `rate_limited` | chamadas demais | espera **no mínimo** `retry_after_seconds` |
| 503 | `unavailable` | Hub ou banco dos robôs fora | mantém o último dado, marca a cápsula como sem conexão, tenta de novo |
| 500 | `server_error` | qualquer outra falha | idem 503 |

**Dispositivo pausado não é erro**: vem em `config.paused` (§4), e `me/tasks` continua respondendo.
**Dado velho não é erro**: vem em `sync.stale` (§3), com os dados que existirem.

---

## 2. `POST /api/gestor/enroll`

Roda **uma vez por PC**, no script de instalação, com o código gerado na tela *Dispositivos* do Hub.
Sem `Authorization`.

**Requisição**

```json
{
  "code": "K7QM-4XPA",
  "device": {
    "hostname": "APICE-PES-014",
    "os": "Windows 11 Pro 23H2",
    "agent_version": "0.1.0",
    "machine_id": "3f1c2a9e-…"
  }
}
```

- `code`: 8 caracteres `[A-Z2-9]` (sem 0/O/1/I), com ou sem hífen, sem diferenciar caixa.
- `machine_id`: o `MachineGuid` do Windows. Serve para o Hub avisar "esta máquina já foi enrolada
  antes" — não é segredo nem autenticação.

**Resposta `201`**

```json
{
  "api": 1,
  "token": "gdt_…",
  "device_id": "dev_01JA…",
  "person": {
    "id": "pes_rafael",
    "display_name": "Rafael Moura",
    "first_name": "Rafael",
    "department": "Pessoal"
  }
}
```

Erros possíveis: `invalid_request`, `invalid_code`, `code_expired`, `code_used`, `excluded_account`, `rate_limited`.

**Regras do código (aprovadas pelo Farol, 08/10/2026):**

- Validade de **24 h**, **uso único**.
- Limite de **10 tentativas por minuto por IP**. O contador vive numa **tabela do Hub**, não em memória
  (a Vercel tem várias instâncias e reinicia).
- ⚠️ **Os 95 PCs saem pelo mesmo IP do escritório.** Por isso o instalador:
  - respeita `retry_after_seconds` do `429` antes de tentar de novo;
  - **nunca repete `invalid_code`, `code_expired` ou `code_used` em laço** — só tenta de novo com
    um código **novo digitado** pela pessoa. Um instalador em laço esgota o limite do escritório
    inteiro.

---

## 3. `GET /api/gestor/me/tasks`

O cartão inteiro numa chamada. O Hub resolve **tudo** que é semântica do Gestta; o agente só desenha.

**Parâmetros:** nenhum obrigatório. `If-None-Match: <etag>` → `304` sem corpo quando nada mudou
(o agente consulta a cada `poll_interval_seconds`; a maioria das respostas deve ser 304).

**ETag** = hash de (**id da pessoa**, `today`, `max(atualizado_em)` global da tabela, versão do de-para,
`config_version`). O `today` entra porque **na virada do dia os buckets mudam sem nenhuma linha mudar**:
a consulta das 00:01 tem que devolver `200` com as vencidas novas, mesmo sem sync. A pessoa entra porque
o resto é igual para todo mundo — sem ela, o ETag de uma pessoa revalida a cópia de outra (§1.3).

**Resposta `200`** (cabeçalho `ETag`)

```json
{
  "api": 1,
  "generated_at": "2026-10-08T13:02:11-03:00",
  "today": "2026-10-08",
  "person": { "id": "pes_rafael", "display_name": "Rafael Moura", "first_name": "Rafael", "department": "Pessoal" },
  "sync": {
    "source": "gestta",
    "last_sync_at": "2026-10-08T13:04:00-03:00",
    "stale": false,
    "stale_after_business_hours": 6
  },
  "summary": {
    "overdue": 6,
    "due_today": 9,
    "done_today": 4,
    "today_total": 19,
    "week": 18,
    "open_total": 146
  },
  "tasks": [
    {
      "id": "8812734",
      "title": "Rescisão — contrato 00482",
      "client": "MM Mercado",
      "department": "Pessoal",
      "competence": "2026-09",
      "status": "open",
      "bucket": "overdue",
      "due_date": "2026-10-06",
      "due_time": null,
      "days_overdue": 2,
      "done_date": null,
      "done_at": null,
      "gestta_url": "https://app.gestta.com.br/#/sidebar/task/8812734"
    }
  ],
  "truncated": false
}
```

### 3.1 Como o Hub monta isso (semântica do Gestta → contrato)

Fonte: `tarefas` no banco dos robôs, lida pelo Hub com `hub_reader`. Pessoa = o(s) nome(s) do
de-para *dispositivo → pessoa do Hub → nome no Gestta* (`tarefa_responsavel`).

| Regra | Origem |
|---|---|
| Entra quem tem `tarefa_status ∈ {OPEN, IMPEDIMENT}` → `status: "open"` / `"impediment"` | §3.3 |
| Entra `DONE` **só se `done_date = today`** → `status: "done"`, `bucket: "today"`. O Gestta só dá o **dia** da conclusão; não depende de `gestor_snapshots` | ponto verde; Farol 08/10 |
| `DISCONSIDERED` nunca entra | §3.3 |
| `cliente_ativo = false` nunca entra (ruído) | §3.3 |
| Conta-caixa nunca recebe tarefas (`403 excluded_account`) | §3.1, §4 |
| `bucket = "overdue"` se aberta e `tarefa_datameta < today` — e só isso. A flag **`Atrasada?`** do Gestta é **ignorada** | §3.3; Farol 08/10 |
| `bucket = "today"` se `tarefa_datameta = today` (aberta ou concluída hoje) | |
| `bucket = "week"` se aberta e `today < tarefa_datameta ≤ today + 7` (dias corridos) | |
| `bucket = "later"` se aberta e `tarefa_datameta > today + 7`, ou sem data | |
| `sync.last_sync_at` = `max(atualizado_em)` **global da tabela `tarefas`** — não das linhas da pessoa. Um sync vale para todos; com rodadas incrementais, quem não teve tarefa mexida apareceria como velho com o robô saudável | §3.3; Farol 08/10 |
| `sync.stale` = horas úteis entre `last_sync_at` e agora `> stale_after_business_hours` (ver §3.5) | guarda de dado velho |
| `due_date` = `tarefa_datameta` como **data de calendário em São Paulo**. O banco dos robôs grava sem fuso e o servidor roda em UTC: **a conversão é do Hub** | Farol 08/10 |
| `department` = `empresa_departamento` do Gestta, passado pelo de-para para o nome do Hub | Farol 08/10 |

**Vencida e hoje são calculadas no momento da consulta, nunca no momento do sync.** `today` é a data
de São Paulo no instante em que o Hub responde; `bucket` e `days_overdue` saem de `tarefa_datameta`
comparada com esse `today`. Uma tarefa sincronizada ontem com meta hoje aparece hoje como `today` — e
amanhã como `overdue`, mesmo que o robô não tenha rodado. O sync só define **o que** existe e o
status; **quando** vence é sempre relativo a agora. Se o Hub mantiver cache, ele guarda as linhas
lidas, não os buckets; nenhum cache atravessa a meia-noite com `today` velho.

### 3.2 As abas, sem o agente pensar

| Aba | Tarefas | Contador da aba |
|---|---|---|
| **Hoje** | `bucket ∈ {overdue, today}` — grupos "Vencidas" (overdue) e "Hoje" (today, abertas antes das concluídas) | `overdue + due_today` |
| **Semana** | `bucket = week`, agrupadas por `due_date` | `week` |
| **Todas** | todas com `status ≠ done`, ordenadas por `due_date` | `open_total` |

- **Cápsula:** `N vencidas` = `summary.overdue`; `N hoje` = `summary.due_today` (só abertas).
- **"Concluídas hoje X de Y"** e anel de progresso: `X = done_today`, `Y = today_total`
  (`today_total = overdue + due_today + done_today`).
- **Busca da aba Todas:** local, sobre `title` + `client`. Não há rota de busca.
- **Ordem:** `tasks` chega ordenado por `due_date` asc, `due_time` asc (nulos no fim), `client`.
- **Etiqueta da linha:** `overdue` → `"{days_overdue}d"`; `today` → `"hoje"` (com `due_time`,
  que na v1 é sempre null, seria `"17h"`); `done` → `"ok"`; `week` → `"qui 9"`; `later` → `"09/11"`.
- **Clicar na linha:** abre `gestta_url` no navegador padrão. O agente não monta URL.

### 3.3 Campos da tarefa

| Campo | Tipo | Nota |
|---|---|---|
| `id` | string | `tarefa_id` do Gestta, como texto |
| `title` | string | nome da tarefa no Gestta |
| `client` | string | nome do cliente, já sem "Ltda"/sufixos se o Hub quiser encurtar |
| `department` | string | `empresa_departamento` do Gestta, já no nome do Hub (de-para resolvido): `Pessoal`, `Fiscal`, `Contábil`, `Legalização`… |
| `competence` | `"YYYY-MM"` \| null | o relatório do Gestta não tem coluna de competência; o Hub tenta uma das 32 colunas ou regex no título, senão `null`. **O agente nunca interpreta o título** |
| `status` | `open` \| `impediment` \| `done` | `impediment` é aberta, com selo "impedida" |
| `bucket` | `overdue` \| `today` \| `week` \| `later` | regra em §3.1 |
| `due_date` | `YYYY-MM-DD` \| null | `tarefa_datameta`, data de calendário em São Paulo (conversão do Hub) |
| `due_time` | `HH:MM` \| null | **sempre `null` na v1**: a meta do Gestta é só dia. Fica no contrato para não quebrar se um dia vier hora |
| `days_overdue` | int | 0 se não vencida; dias corridos |
| `done_date` | `YYYY-MM-DD` \| null | **garantido em `status: done`**; `null` nos outros |
| `done_at` | instante \| null | opcional; `null` quando o Gestta só tem o dia (o caso da v1) |
| `gestta_url` | string | link direto para a tarefa, montado pelo Hub. ⚠️ **Formato ainda é placeholder**: o Gestta é SPA AngularJS com rota em hash; o mock usa `https://app.gestta.com.br/#/sidebar/task/<id>`. O Farol confirma abrindo uma tarefa numa sessão logada antes de implementar |

### 3.4 Volume

Mediana de 146 abertas por pessoa; 4 pessoas passam de 100 só em "vencidas + hoje". O Hub devolve
**tudo** até 2.000 tarefas por pessoa. Acima disso corta as `later` mais distantes e marca
`"truncated": true` (o agente mostra "mostrando 2.000 de N" na aba Todas). Contas-caixa (milhares)
já estão fora por regra.

### 3.5 Dado velho (decisão do José Luiz, 08/10/2026)

- **A unidade é hora útil:** só conta o tempo de **segunda a sexta, das 08:00 às 18:00** (São Paulo).
  Noite e fim de semana **não contam**. Feriado, na v1, conta como dia útil.
- **Limite inicial: 6 horas úteis**, configurável na administração do Hub (*Configurações do agente*).
- `stale = horas_uteis(last_sync_at, agora) > stale_after_business_hours`, calculado pelo Hub a cada
  consulta. O agente não refaz a conta; usa `sync.stale` e mostra `last_sync_at`.
- Exemplos com limite 6: sync sexta 16:00 → segunda 09:00 = 3 h úteis (fresco). Sync terça 10:00,
  rodadas de 13:00 e 16:00 falham → às 16:01 são 6 h01 úteis (velho).
- **Rodadas do robô:** fixas às **07:00, 10:00, 13:00 e 16:00**. O mock simula `last_sync_at` nesses
  horários até isso estar em produção.

---

## 4. `GET /api/gestor/me/config`

O que a tela *Configurações do agente* do Hub decidiu para esta máquina, já com a precedência
**pessoa > departamento > geral** resolvida. O departamento dessa precedência é o **da pessoa no Hub**
(de-para), não o das tarefas dela. O agente busca ao iniciar, quando o heartbeat disser que
mudou (`config_version`) e, no mínimo, a cada hora. Suporta `ETag`/`304`.

**Resposta `200`**

```json
{
  "api": 1,
  "config_version": "c-2026-10-08T09:00:00Z",
  "paused": false,
  "tabs": ["hoje", "semana", "todas"],
  "default_tab": "hoje",
  "morning": { "enabled": true, "duration_seconds": 10, "once_per_day": true, "not_before": "07:00" },
  "stale_after_business_hours": 6,
  "sync_times": ["07:00", "10:00", "13:00", "16:00"],
  "poll_interval_seconds": 600,
  "heartbeat_interval_seconds": 900,
  "notice": null,
  "update": { "min_agent_version": "0.1.0", "force": false }
}
```

| Campo | Efeito no agente |
|---|---|
| `paused` | `true` → cápsula some (fica só o ícone da bandeja); continua mandando heartbeat |
| `tabs` | abas visíveis, na ordem; sempre contém `default_tab` |
| `morning` | bom-dia ao iniciar a sessão do Windows: aberto por `duration_seconds`, no máximo 1×/dia se `once_per_day`, e só depois de `not_before` (hora local) |
| `stale_after_business_hours` | o mesmo valor de `sync.stale_after_business_hours`, em **horas úteis** (§3.5) — a decisão de "velho" é do servidor; aqui é só para o texto do aviso |
| `sync_times` | horários das rodadas do robô (`HH:MM`, São Paulo), cadastrados na administração do Hub. A interface monta o rodapé a partir dele (`length` → "4×/DIA"), sem número fixo no HTML. Lista vazia = horário desconhecido: o rodapé não fala de cadência. Informativo: não entra na conta de `stale`. O agente **pode** buscar `me/tasks` alguns minutos depois de cada horário, **além** do `poll_interval_seconds`, nunca no lugar dele: a virada do dia e a passagem para "velho" acontecem sem sync |
| `poll_interval_seconds` | intervalo de `GET me/tasks` (mínimo aceito pelo agente: 60) |
| `heartbeat_interval_seconds` | intervalo do heartbeat (mínimo: 300) |
| `commands_poll_interval_seconds` | intervalo de `GET me/commands`; `0` = desligado. **Proposta, §8.4** |
| `notice` | `null` ou `{ "level": "info"\|"warn", "text": "…" }` — faixa no topo do cartão |
| `update.min_agent_version` | abaixo disso o agente força o updater; `force: true` força mesmo acima |

---

## 5. `POST /api/gestor/heartbeat`

Telemetria. **Só isto** — nada de tela, teclado, aplicativo ou janela (Arquitetura §3.1 e §5).

**Requisição**

```json
{
  "sent_at": "2026-10-08T13:05:00-03:00",
  "agent_version": "0.1.0",
  "os": "Windows 11 Pro 23H2",
  "config_version": "c-2026-10-08T09:00:00Z",
  "last_tasks_ok_at": "2026-10-08T13:02:11-03:00",
  "panel_opens_since_last": 2,
  "morning_shown_at": "2026-10-08T08:03:12-03:00",
  "last_error_code": null,
  "last_command_id": null,
  "uptime_seconds": 5312
}
```

`last_command_id` e `uptime_seconds`: **proposta, §8.5**. O agente 0.1.0 não manda; o Hub aceita sem.

- `panel_opens_since_last`: quantas vezes o cartão foi aberto pelo usuário desde o último heartbeat
  (alimenta "quem nunca abriu o painel").
- `morning_shown_at`: último bom-dia exibido, ou `null`.
- `last_error_code`: último `error.code` recebido, ou `null`.

**Resposta `200`**

```json
{ "api": 1, "received_at": "2026-10-08T13:05:00-03:00", "config_version": "c-2026-10-08T09:00:00Z" }
```

Se `config_version` da resposta for diferente do enviado, o agente busca `me/config` na hora.
Falha de heartbeat **nunca** aparece para o usuário; o agente tenta de novo no próximo ciclo.

---

## 6. Respostas do Farol (08/10/2026) e o que falta

O Farol respondeu sem acesso ao banco dos robôs (`hub_reader` ainda não existe). Já estão no texto:

1. `gestta_url` — rota em hash; placeholder `…/#/sidebar/task/<id>` (§3.3).
2. Conclusão — só o dia: `done_date` garantido, `done_at` opcional; "concluída hoje" = `done_date = today` (§3.1, §3.3).
3. `tarefa_datameta` é dia — `due_time` sempre `null` na v1; fuso resolvido pelo Hub (§3.1, §3.3).
4. Competência — sem coluna; Hub tenta coluna ou regex; agente não interpreta título (§3.3).
5. Código de enrolamento — 24 h, uso único, 10/min por IP em tabela do Hub (§2).

Ajustes ao desenho, também já no texto: ETag com `today` (§3), `last_sync_at` global (§3.1),
precedência pelo departamento da pessoa (§4), `department` de `empresa_departamento` (§3.1), flag
`Atrasada?` ignorada (§3.1). Token `gdt_` com SHA-256, erros e precedência: aprovados sem mudança.

**Ainda falta:**
- formato real do `gestta_url` (Farol confere numa sessão logada);
- a lista das 32 colunas de `tarefas`, que vem pelo Diretor — pode mudar a origem de `competence` e
  de `done_date`.

---

## 7. Mock local

`npm run mock` (ou `node mock/server.mjs`) sobe o contrato inteiro em `http://127.0.0.1:8787`, com
dados fictícios no tamanho dos números reais (§4 da Arquitetura). Tokens, códigos e cenários estão em
`mock/README.md`. `npm run mock:smoke` confere todas as rotas.

---

## 8. Ordens ao dispositivo — desligar o PC pelo Hub

> **Proposta** (Ponte, 09/10/2026), a partir de `~/dev/Jarvis/PROJETOS/Apice-Hub/Logs/2026-10-09-pedido-gestor-desligar-pc.md`.
> Desenho com "manda bala" do José Luiz; **contrato aguardando a aprovação dele**, depois vai ao Farol
> e libera o Chassi. Muda o escopo da Arquitetura (§1 e §5 diziam que o agente não controla a
> máquina) — o registro lá é do Vigia.

**Mudança compatível:** rotas e campos novos, `X-Gestor-Api: 1` continua. O agente 0.1.0 ignora
`commands_poll_interval_seconds` e nunca chama as rotas novas.

### 8.1 Por que existe e o que não pode voltar

O Hub Desktop desliga só os PCs que estão com ele aberto (38 de 98 em 09/10); o Ápice Gestor vai estar
em todos. **O laço de junho/2026 não pode voltar:** o `shutdown /t 0` apagava o PC antes de a
confirmação chegar ao servidor, a ordem continuava pendente e o PC do Vinny desligava de novo a cada
vez que o app abria. Aqui a ordem **só é executada depois de reivindicada no servidor**, uma vez, e
nenhuma resposta incerta leva a desligar.

### 8.2 Rotas

| Rota | Função |
|---|---|
| `GET  /api/gestor/me/commands` | ordens pendentes e não vencidas **deste dispositivo** |
| `POST /api/gestor/me/commands/{id}/claim` | reivindica a ordem, de forma atômica; só `200` autoriza executar |
| `POST /api/gestor/me/commands/{id}/result` | avisa que a execução **falhou** (o PC continuou ligado) |

Todas com o token de dispositivo (§1.2). **A ordem é por dispositivo, nunca por pessoa:** no piloto,
o PC do Vinny está vinculado ao José — por pessoa, desligaria o PC errado. O dispositivo é o do token.

Cabeçalhos: como §1.3, mas **sem ETag e com `Cache-Control: private, no-store`** em todas as
respostas destas rotas (ordem nunca sai de cache) e `Vary: Authorization`.

#### `GET /api/gestor/me/commands`

Chamada a cada `commands_poll_interval_seconds` (§8.4). Lista vazia é o caso normal.

```json
{ "api": 1, "commands": [ { "id": "cmd_01JB…", "type": "shutdown", "age_seconds": 12, "expires_in_seconds": 288 } ] }
```

| Campo | Nota |
|---|---|
| `id` | opaco, `cmd_` + texto; único para sempre (não reaproveitar) |
| `type` | `shutdown` na v1. Tipo desconhecido: o agente **ignora** (não reivindica) |
| `age_seconds` | idade da ordem **pelo relógio do servidor**, no instante da resposta. O agente nunca usa o relógio do PC |
| `expires_in_seconds` | quanto falta para vencer (§8.3) |

Só entram ordens `pending` e não vencidas, deste dispositivo, da mais velha para a mais nova.

#### `POST /api/gestor/me/commands/{id}/claim`

Corpo vazio (`{}`). Transição atômica `pending → claimed`, uma vez só (no Hub: `UPDATE … WHERE id = $1
AND device_id = $2 AND state = 'pending' AND created_at > now() - interval '5 min' RETURNING …`).

- **`200`** `{ "api": 1, "id": "cmd_…", "type": "shutdown", "claimed_at": "2026-10-09T14:02:11-03:00" }`
  → **pode executar.**
- **`409 command_gone`** → já reivindicada, vencida, cancelada, de outro dispositivo ou inexistente
  (o Hub não distingue, para não vazar ordem de outra máquina) → **não executa.**
- Qualquer outra coisa (rede, timeout, `5xx`, `401`, corpo ilegível) → **não executa** e **não tenta
  de novo** (trava a). Se o claim chegou ao Hub e a resposta se perdeu, a ordem fica `claimed` e nunca
  mais é entregue: o admin vê "reivindicada, sem confirmação" e manda outra se quiser.

#### `POST /api/gestor/me/commands/{id}/result`

Só para **falha**: o agente rodou a sequência de desligamento e continua vivo.

```json
{ "status": "failed", "error": "shutdown.exe 1190; Stop-Computer: acesso negado; WMI 5: código 1191" }
```

- `status`: só `failed` na v1. `error`: texto curto, até 500 caracteres, **sem token nem dado pessoal**.
- `200` `{ "api": 1, "id": "cmd_…", "state": "failed" }`. `409 command_gone` se a ordem não está
  `claimed` por este dispositivo. `400 invalid_request` se o corpo não bate.
- **Sucesso não se avisa** (o PC apagou): fecha-se pelo heartbeat (§8.5).

### 8.3 Estados da ordem (no Hub)

```
pending ──claim──▶ claimed ──heartbeat pós-boot──▶ executed
   │                  └──result failed──────────▶ failed
   ├──5 min sem claim──▶ expired
   └──admin cancela────▶ canceled
```

- **Vence em 5 minutos** sem claim e **nunca mais é entregue** (PC desligado ou sem rede quando o
  admin mandou). O vencimento é checado também dentro do `claim`, não só na listagem.
- **No máximo uma `shutdown` pendente por dispositivo:** pedir de novo enquanto há uma pendente devolve
  a mesma ordem (não cria outra). "Mandar duas vezes" desliga uma vez só.
- `claimed` sem fechamento em 24 h → o admin vê "sem confirmação". Nunca volta para `pending`.
- `executed`, `failed`, `expired`, `canceled` são finais.

### 8.4 Campo novo em `me/config`

| Campo | Efeito |
|---|---|
| `commands_poll_interval_seconds` | intervalo de `GET me/commands`. Proposta: **60**. **`0` = desligado**: o agente não consulta ordens. Mínimo aceito pelo agente quando ligado: 30 |

Independe de `paused`: dispositivo pausado continua recebendo ordem (pausa esconde a cápsula, não tira
a máquina da administração). Para tirar, o admin põe `0`.

### 8.5 Campos novos no heartbeat

| Campo | Nota |
|---|---|
| `last_command_id` | id da última ordem que este dispositivo **executou** (gravada antes de desligar, trava c), ou `null` |
| `uptime_seconds` | tempo ligado do Windows (`GetTickCount64() / 1000`), inteiro |

**Fechamento como `executed`:** o Hub marca `executed` quando recebe um heartbeat deste dispositivo com
`last_command_id` = uma ordem `claimed` **e** `uptime_seconds < agora − claimed_at` (o Windows foi
ligado depois do claim, então desligou mesmo). Sem a segunda condição, um PC que falhou em desligar e
não conseguiu mandar o `result` seria registrado como desligado. Nosso desligamento é completo
(`shutdown /s`, sem `/hybrid`), então o uptime zera.

### 8.6 Regras do agente — as quatro travas contra o laço

São regras de **cliente**: o Hub não tem como garanti-las. Valem desde o primeiro segundo da
execução — com o início automático, toda ligada do PC abre o Gestor.

- **(a) Só executa com `claim` = `200` nesta execução do app.** Erro de rede, `409` ou qualquer outra
  resposta → não executa e não fica tentando. Claim de uma execução anterior não vale.
- **(b) Ordem anterior ao início desta execução é ignorada:** se `age_seconds` > tempo desde que **este
  processo** começou (relógio monotônico do processo, ex.: `Instant` no Rust), a ordem é velha → não
  reivindica e não executa. Compara duração com duração; não depende do relógio do PC.
  *Ajuste da Ponte ao desenho, que comparava com o uptime do Windows:* com a **Inicialização
  Rápida** (padrão do Windows 10/11), desligar pelo menu Iniciar hiberna o kernel e o uptime pode não
  zerar — uma ordem criada com o PC "desligado" passaria pela trava. O tempo do processo é sempre ≤ o
  uptime, então a regra fica mais estrita, nunca mais frouxa. Custo: uma ordem que chega enquanto o
  app reinicia (atualização) é ignorada; o admin manda de novo. **A confirmar na bancada** (aceite 7).
- **(c) Grava o `id` localmente ANTES de desligar** (arquivo no AppData do usuário, ex.:
  `%APPDATA%\ApiceGestor\commands.json`, com escrita atômica). Id já gravado **nunca** é executado de
  novo, mesmo que reapareça. Se a gravação falhar → não executa e manda `result failed`.
- **(d) Uma tentativa por ordem:** `shutdown /s /f /t 0 /c "Apice Gestor: admin requisitou desligar"`
  → se falhar, `Stop-Computer -Force` → se falhar, WMI `Win32Shutdown(5)`. Uma vez cada, sem laço,
  usuário comum, sem aviso na tela (decisão do José Luiz). Se o processo ainda estiver vivo **60 s**
  depois da última tentativa, manda `result failed` com o motivo.

Ordem de operações para cada ordem recebida:

```
type == shutdown?  ──não──▶ ignora
id já em commands.json? ──sim──▶ ignora
age_seconds > uptime do processo? ──sim──▶ ignora            (trava b)
POST claim ── ≠ 200 ──▶ não executa, não repete               (trava a)
grava id em commands.json ── falhou ──▶ result failed, fim    (trava c)
shutdown.exe → Stop-Computer → WMI, uma vez                   (trava d)
vivo 60 s depois? ──▶ result failed
```

No máximo **uma** ordem de desligar por consulta. Token nunca em log; o `id` da ordem pode ir para o
log.

### 8.7 Critérios de aceite (bancada, com o José Luiz presente)

1. Admin manda desligar → o PC apaga em até ~1 min, sem aviso.
2. Ligar de novo e abrir o Gestor → **não** desliga (repetir 3 vezes).
3. PC desligado quando a ordem foi criada, ligado 10 min depois → não desliga; histórico "vencida".
4. Ordem mandada duas vezes seguidas → desliga uma vez só.
5. Rede caída na hora do claim → não desliga.
6. Desligar e ligar sem abrir nada → o Gestor sobe sozinho (início automático); repetir depois de
   desativar em Gerenciador de Tarefas → Aplicativos de inicialização e abrir o app uma vez. *(Casca,
   não contrato.)*
7. **(novo, da trava b)** Com a Inicialização Rápida **ligada**: desligar o PC pelo menu Iniciar, admin
   manda a ordem, ligar o PC em até 2 min → **não** desliga.
8. **(novo)** Bloquear a escrita em `commands.json` → não desliga e o histórico mostra "falhou".

### 8.8 Erros

Padrão do §1.4. Código novo: `409 command_gone` — "Esta ordem não está mais disponível." O agente não
mostra ao usuário; só registra.
