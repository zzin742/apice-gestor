# Mock da API do Ápice Gestor

Serve `docs/contrato-api.md` (v1) em `http://127.0.0.1:8787/api/gestor`, sem dependências.
Dados **fictícios** (nenhuma pessoa ou cliente real), no tamanho dos números de 07/10/2026.

`last_sync_at` simula as rodadas fixas do robô (07h, 10h, 13h, 16h, alguns minutos depois do
horário cheio). `sync.stale` usa horas úteis (seg–sex 08–18) com limite de 6 — contrato §3.5.
`bucket` e `days_overdue` são calculados na hora da consulta.

```sh
npm run mock                         # sobe na 8787
MOCK_NOW=2026-10-08T13:02 npm run mock   # relógio fixo (prints, testes)
MOCK_PORT=9000 npm run mock
npm run mock:smoke                   # sobe numa porta aleatória, confere tudo, derruba
```

Da interface, use o cliente: `createClient({ baseUrl: MOCK_BASE_URL, token: 'gdt_mock_rafael' })`
em `src/api/client.js`. `GET /__mock` (fora do contrato) lista tokens, códigos e heartbeats recebidos.

## Tokens (`Authorization: Bearer …`)

| Token | Cenário |
|---|---|
| `gdt_mock_rafael` | **pessoa típica** (Pessoal): 6 vencidas + 9 hoje = 15 (a mediana), 4 concluídas hoje, 18 na semana, 146 abertas |
| `gdt_mock_bianca` | **carteira pesada** (Fiscal): 118 vencidas, 23 hoje, 512 abertas |
| `gdt_mock_caio` | **dia limpo** (Contábil): nada vencido nem hoje, 1 concluída |
| `gdt_mock_denise` | **dado velho**: última rodada a mais de 12 horas úteis → `sync.stale = true` e `notice` na config |
| `gdt_mock_elton` | **pausado** no Hub → `config.paused = true` |
| `gdt_mock_revogado` | `401 token_revoked` |
| `gdt_mock_sem_depara` | `409 person_not_mapped` |
| `gdt_mock_caixa` | `403 excluded_account` |
| `gdt_mock_fora` | `503 unavailable` |
| `gdt_mock_velho_agente` | `426 upgrade_required` |

## Códigos de enrolamento

| Código | Resultado |
|---|---|
| `K7QM-4XPA` | 201 → Rafael (token novo, vale até reiniciar o mock) |
| `B3NT-9ZQW` | 201 → Bianca |
| `C4UN-2SLM` | 201 → Caio |
| `VENC-7DQX` | 410 `code_expired` |
| `USAD-2222` | 409 `code_used` |
| `CAXA-4444` | 403 `excluded_account` |
| qualquer outro | 400 `invalid_code` |

Os tokens `gdt_mock_*` não são segredo: só existem aqui. Token de verdade nunca entra em repo.
