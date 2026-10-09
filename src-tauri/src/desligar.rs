//! Terreno para o desligamento do PC por ordem do Hub
//! (Jarvis: PROJETOS/Apice-Hub/Logs/2026-10-09-pedido-gestor-desligar-pc.md).
//!
//! NADA AQUI É CHAMADO AINDA. Espera o contrato das rotas (Ponte) e o "manda bala" do José Luiz.
//! Estão prontas as travas que não dependem do contrato:
//!   (b) ordem anterior ao boot é ignorada — `anterior_ao_boot`, com o tempo ligado monotônico;
//!   (c) id gravado ANTES de desligar e nunca executado de novo — `Executados`;
//!   (d) uma tentativa só da sequência shutdown → Stop-Computer → WMI — `desligar`.
//! A trava (a), claim 200 nesta execução, depende das rotas e fica para depois.
#![allow(dead_code)] // até o contrato fechar

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Quantos ids guardar; ordens vencem em minutos, então isso cobre muito mais que o necessário.
const MAXIMO_GUARDADOS: usize = 500;
const ARQUIVO: &str = "comandos-executados.json";

/// Há quanto tempo o Windows está ligado (`GetTickCount64`: monotônico, não depende do relógio).
#[cfg(windows)]
pub fn tempo_ligado() -> Option<Duration> {
    let ms = unsafe { windows_sys::Win32::System::SystemInformation::GetTickCount64() };
    Some(Duration::from_millis(ms))
}

#[cfg(not(windows))]
pub fn tempo_ligado() -> Option<Duration> {
    None
}

/// Trava (b): a ordem tem mais idade (relógio do servidor) do que o Windows tem de ligado?
/// Então foi criada antes deste boot e não vale. Sem tempo ligado confiável, trata como anterior.
pub fn anterior_ao_boot(age_seconds: u64) -> bool {
    match tempo_ligado() {
        Some(ligado) => age_seconds > ligado.as_secs(),
        None => true,
    }
}

#[derive(Serialize, Deserialize)]
struct Executado {
    id: String,
    em: String,
}

/// Trava (c): ids de ordens já executadas, em `%LOCALAPPDATA%\br.com.apice.gestor\`.
pub struct Executados {
    arquivo: PathBuf,
}

impl Executados {
    pub fn em(pasta: &Path) -> Self {
        Self { arquivo: pasta.join(ARQUIVO) }
    }

    /// Já executado? Arquivo ausente = nada executado. Arquivo ilegível = responde **sim**:
    /// sem conseguir provar que não rodou, não roda.
    pub fn contem(&self, id: &str) -> bool {
        match std::fs::read_to_string(&self.arquivo) {
            Ok(texto) => match serde_json::from_str::<Vec<Executado>>(&texto) {
                Ok(lista) => lista.iter().any(|e| e.id == id),
                Err(_) => true,
            },
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(_) => true,
        }
    }

    /// Grava o id e só volta depois de o arquivo estar no disco (`sync_all`). Chamar ANTES de
    /// desligar; se falhar, não desliga.
    pub fn registrar(&self, id: &str) -> io::Result<()> {
        let mut lista: Vec<Executado> = std::fs::read_to_string(&self.arquivo)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        lista.push(Executado { id: id.to_owned(), em: crate::api::agora() });
        let excesso = lista.len().saturating_sub(MAXIMO_GUARDADOS);
        lista.drain(..excesso);

        if let Some(pasta) = self.arquivo.parent() {
            std::fs::create_dir_all(pasta)?;
        }
        let temporario = self.arquivo.with_extension("json.tmp");
        let mut f = std::fs::File::create(&temporario)?;
        f.write_all(serde_json::to_string_pretty(&lista)?.as_bytes())?;
        f.sync_all()?;
        drop(f);
        std::fs::rename(&temporario, &self.arquivo)?;
        std::fs::File::open(&self.arquivo)?.sync_all()
    }
}

/// Trava (d): desliga o PC agora, sem aviso, com usuário comum. Cada passo uma vez só, sem laço;
/// passa ao próximo só se o anterior falhar. Mesmos comandos do Hub Desktop.
/// Devolve `Ok` quando um passo foi aceito (o PC vai apagar); `Err` com os motivos se todos falharam.
#[cfg(windows)]
pub fn desligar() -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    const SEM_JANELA: u32 = 0x0800_0000; // CREATE_NO_WINDOW

    // Caminhos absolutos em System32: nada de PATH.
    let raiz = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".into());
    let shutdown = format!(r"{raiz}\System32\shutdown.exe");
    let powershell = format!(r"{raiz}\System32\WindowsPowerShell\v1.0\powershell.exe");

    let passos: [(&str, Vec<&str>); 3] = [
        (&shutdown, vec!["/s", "/f", "/t", "0", "/c", "Apice Gestor: admin requisitou desligar"]),
        (&powershell, vec!["-NoProfile", "-NonInteractive", "-Command", "Stop-Computer -Force"]),
        (
            &powershell,
            vec![
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "(Get-WmiObject Win32_OperatingSystem -EnableAllPrivileges).Win32Shutdown(5)",
            ],
        ),
    ];

    let mut falhas = Vec::new();
    for (programa, args) in passos {
        match Command::new(programa).args(&args).creation_flags(SEM_JANELA).status() {
            Ok(s) if s.success() => return Ok(()),
            Ok(s) => falhas.push(format!("{programa}: saiu com {s}")),
            Err(e) => falhas.push(format!("{programa}: {e}")),
        }
    }
    Err(falhas.join("; "))
}

/// Fora do Windows não desliga nada (o Mac é só de desenvolvimento).
#[cfg(not(windows))]
pub fn desligar() -> Result<(), String> {
    Err("desligamento só existe no Windows".into())
}
