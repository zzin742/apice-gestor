//! Terreno para o desligamento do PC por ordem do Hub
//! (Jarvis: PROJETOS/Apice-Hub/Logs/2026-10-09-pedido-gestor-desligar-pc.md).
//!
//! Contrato §8. As quatro travas contra o laço estão aqui e no laço de `comandos.rs`:
//!   (a) claim 200 nesta execução — `api::Api::claim` (em comandos.rs);
//!   (b) ordem anterior a esta execução é ignorada — `anterior_a_esta_execucao`, pelo uptime
//!       DO PROCESSO (§8.6: a Inicialização Rápida pode não zerar o uptime do Windows);
//!   (c) id gravado ANTES de desligar e nunca executado de novo — `Executados`;
//!   (d) uma tentativa só da sequência shutdown → Stop-Computer → WMI — `desligar`.

use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Marco do início "desta execução" para a trava (b). É RESETÁVEL: além do start do serviço, o
/// resume de energia (Fast Startup/suspensão) rezera — senão o relógio contaria o tempo em que o
/// PC esteve desligado e uma ordem criada nesse período passaria pela trava (crítério 7).
static INICIO: Mutex<Option<Instant>> = Mutex::new(None);

/// Chamar no start do serviço e a cada retomada de energia. Rezera o relógio da trava (b).
pub fn marcar_inicio() {
    *INICIO.lock().unwrap() = Some(Instant::now());
}

/// Quanto tempo desde o último `marcar_inicio` (relógio monotônico). Sem marca ainda → zero, e a
/// trava (b) ignora tudo (lado seguro: sem relógio, não desliga).
fn uptime_processo() -> Duration {
    INICIO.lock().unwrap().map(|i| i.elapsed()).unwrap_or(Duration::ZERO)
}

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

/// `uptime_seconds` do heartbeat (§8.5): tempo ligado do Windows em segundos, ou 0 fora do Windows.
pub fn uptime_windows_segundos() -> u64 {
    tempo_ligado().map(|d| d.as_secs()).unwrap_or(0)
}

/// Trava (b): a ordem é mais velha que esta execução do app? Compara a idade da ordem (relógio do
/// servidor) com o uptime DO PROCESSO. Igual-ou-mais-nova passa; mais velha é de antes e é ignorada.
pub fn anterior_a_esta_execucao(age_seconds: u64) -> bool {
    age_seconds > uptime_processo().as_secs()
}

#[derive(Serialize, Deserialize)]
struct Executado {
    id: String,
    em: String,
}

/// Trava (c): ids de ordens já executadas. No serviço, a pasta vem de `cofre::pasta_dados()` —
/// `%ProgramData%\ApiceGestor\comandos-executados.json` (mesma do cofre, ACL fechada).
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

    /// Id da última ordem executada (§8.5, `last_command_id`), ou `None`. Gravado antes de
    /// desligar, sobrevive ao boot; é o que o heartbeat manda para o Hub fechar como executada.
    pub fn ultimo(&self) -> Option<String> {
        let texto = std::fs::read_to_string(&self.arquivo).ok()?;
        let lista: Vec<Executado> = serde_json::from_str(&texto).ok()?;
        lista.last().map(|e| e.id.clone())
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

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn executados_nao_repete() {
        // Trava (c): id gravado antes de desligar nunca roda de novo.
        let pasta = std::env::temp_dir().join(format!("apice-exec-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&pasta);
        let ex = Executados::em(&pasta);
        assert!(!ex.contem("cmd_1"));
        ex.registrar("cmd_1").unwrap();
        assert!(ex.contem("cmd_1"));
        assert_eq!(ex.ultimo().as_deref(), Some("cmd_1"));
        // reaparecer o mesmo id: segue marcado como executado
        assert!(ex.contem("cmd_1"));
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn trava_b_ignora_ordem_anterior() {
        // Sem marca de início → uptime 0 → qualquer ordem com idade > 0 é anterior (ignora).
        *INICIO.lock().unwrap() = None;
        assert!(anterior_a_esta_execucao(1));
        // Com o relógio zerado agora, uma ordem "velha" (1h) é anterior; uma de 0s não é.
        marcar_inicio();
        assert!(anterior_a_esta_execucao(3600));
        assert!(!anterior_a_esta_execucao(0));
    }
}
