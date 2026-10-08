# Ápice Gestor

Aplicativo de desktop (Windows) para os colaboradores da Ápice: uma cápsula discreta no canto da
tela com as tarefas do dia (fonte: Gestta, via API do Ápice Hub). Só leitura — a baixa continua
sendo no Gestta. Alimenta a escalação ao gestor por WhatsApp quando tarefa vence.

- **Arquitetura e decisões:** `~/dev/Jarvis/PROJETOS/Apice-Gestor/Arquitetura.md`
- **Interface aprovada:** `design/painel-do-dia.html` (abra no navegador)
- **Regras do projeto:** `AGENTS.md`

Stack: Tauri v2 · interface web · build do `.msi` no GitHub Actions · painel de controle no Ápice Hub.

Estado: nascendo (07/10/2026). Nada instalado em máquina de colaborador.

## Rodar no Mac

```sh
npm install
npm run dev        # = tauri dev (precisa de rustup/cargo)
```

- `src/` — interface servida pela casca (hoje: cópia do protótipo aprovado).
- `src-tauri/` — casca Tauri v2 (Rust e configuração).
