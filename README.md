# Ápice Gestor

Aplicativo de desktop (Windows) para os colaboradores da Ápice: uma cápsula discreta no canto da
tela com as tarefas do dia (fonte: Gestta, via API do Ápice Hub). Só leitura — a baixa continua
sendo no Gestta. Alimenta a escalação ao gestor por WhatsApp quando tarefa vence.

- **Arquitetura e decisões:** `~/dev/Jarvis/PROJETOS/Apice-Gestor/Arquitetura.md`
- **Interface aprovada:** `src/painel.html` + `src/painel.css` — cápsula vermelha (15/10/2026) e cartão
  vermelho e branco (16/10/2026), os dois aprovados pelo José Luiz em preview. Para ver no navegador:
  `npm run mock` e abra `src/index.html` (protótipo de revisão). `design/painel-do-dia.html` é o
  protótipo anterior (07/10), guardado como histórico.
- **Regras do projeto:** `AGENTS.md`

Stack: Tauri v2 · interface web · build do `.msi` no GitHub Actions · painel de controle no Ápice Hub.

Estado: nascendo (07/10/2026). Nada instalado em máquina de colaborador.

## Desenvolver no Mac

```sh
npm install
cd src-tauri && cargo check    # e cargo build — sem executar
```

⚠️ **Não abra a casca no Mac do José Luiz** (`npm run dev`/`tauri dev`, `cargo run`, o binário) sem o OK
dele, pedido pelo Vigia: a janela transparente e sempre no topo travou a máquina em 08/10/2026, e no
macOS ela some ao ser redimensionada. A interface se testa no navegador; o comportamento da janela, no
Windows (bancada).

- `src/` — interface: `painel.html` é a janela da casca; `index.html` é o protótipo de revisão.
- `src-tauri/` — casca Tauri v2 (Rust e configuração).

## Instalação no Windows

O `.msi` instala para a máquina toda (precisa de administrador) em
`C:\Program Files\Ápice Gestor\apice-gestor.exe`. Se a máquina já teve uma instalação anterior, o
instalador reaproveita a pasta registrada em `HKCU\Software\<fabricante>\Ápice Gestor\InstallDir`.
O WebView2 é baixado na instalação se faltar (precisa de internet).

## Enrolamento

Uma vez por PC, no script de instalação, com o código gerado na tela *Dispositivos* do Hub:

```powershell
apice-gestor.exe --enrolar K7QM-4XPA
```

- Troca o código pelo token do dispositivo (`POST /api/gestor/enroll`, contrato §2) no endereço
  compilado no build (`GESTOR_API_BASE`). Não abre janela.
- O token fica no **Credential Manager do Windows** — serviço `br.com.apice.gestor`, conta
  `token-dispositivo` — do **usuário que rodou o comando**: rode como o colaborador, não com outra
  conta de administrador. No Mac de desenvolvimento, vai para o Keychain. Nunca em arquivo nem em log.
- **Sucesso:** imprime `Nome · Departamento` e sai com `0`.
- **Erro:** imprime `code: mensagem` e sai com:

  | Saída | `code` | O que fazer |
  |---|---|---|
  | 2 | `invalid_code`, `code_expired`, `code_used` | pedir **código novo** ao admin; nunca repetir em laço |
  | 3 | `rate_limited` | já esperou `retry_after_seconds` até 3 vezes; tentar mais tarde |
  | 4 | `unavailable`, `server_error` | sem conexão com o Hub ou endereço não configurado |
  | 5 | outros (`invalid_request`, `excluded_account`, cofre de credenciais) | ler a mensagem |
  | 64 | — | faltou o código |

- O executável é de janela: no PowerShell, chame com
  `Start-Process -Wait -PassThru -NoNewWindow -RedirectStandardOutput <arquivo>` e leia `ExitCode`.
