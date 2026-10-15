//! 4/4: o serviço (SYSTEM, Sessão 0) lança a cápsula na sessão interativa do usuário, com o
//! **token do usuário** (nunca SYSTEM — webview como SYSTEM é privilégio total). Reabre se cair,
//! com freio contra laço de respawn. Encerra a cápsula quando o serviço é parado (H3). Laço
//! síncrono numa thread dedicada do serviço.
//!
//! Piloto: acompanha a sessão do **console** (WTSGetActiveConsoleSessionId). Multi-sessão RDP e o
//! SESSION_CHANGE do SCM são evolução pós-piloto.
//! Só Windows; o comportamento real (token/desktop/ambiente) só a bancada confirma.
#![cfg(windows)]

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, TerminateProcess, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT,
    PROCESS_INFORMATION, STARTUPINFOW,
};

const SEM_SESSAO: u32 = 0xFFFF_FFFF;
const INTERVALO: Duration = Duration::from_secs(5);
/// Passo curto para reagir ao STOP sem esperar o intervalo inteiro.
const PASSO: Duration = Duration::from_millis(500);
/// Freio (W4/H2): no máximo tantas PARTIDAS BEM-SUCEDIDAS numa janela, senão pausa.
const MAX_PARTIDAS: usize = 3;
const JANELA: Duration = Duration::from_secs(300);
const PAUSA_APOS_FREIO: Duration = Duration::from_secs(600);

/// Por que `lancar` não lançou.
enum FalhaLancar {
    /// Sessão de console existe mas sem usuário (tela de logon): não é queda, não conta no freio.
    SemUsuario,
    Outro(String),
}

struct Handle(HANDLE);
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

struct Capsula(HANDLE);
impl Capsula {
    fn viva(&self) -> bool {
        unsafe { WaitForSingleObject(self.0, 0) == WAIT_TIMEOUT }
    }
    /// H3: encerra a cápsula que o serviço lançou (ao parar o serviço).
    fn matar(&self) {
        unsafe { TerminateProcess(self.0, 0) };
    }
}
impl Drop for Capsula {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

/// Dorme `total` em passos, checando o sinal de parada. Devolve `true` se foi mandado parar.
fn dormir_ou_parar(parando: &AtomicBool, total: Duration) -> bool {
    let mut restante = total;
    while restante > Duration::ZERO {
        if parando.load(Ordering::SeqCst) {
            return true;
        }
        let passo = restante.min(PASSO);
        std::thread::sleep(passo);
        restante -= passo;
    }
    parando.load(Ordering::SeqCst)
}

pub fn lancar_loop(capsula_exe: PathBuf, parando: Arc<AtomicBool>) {
    let mut atual: Option<Capsula> = None;
    let mut partidas: Vec<Instant> = Vec::new();
    let mut sessao_atual = SEM_SESSAO;
    let mut ultimo_log: &'static str = "";
    loop {
        if dormir_ou_parar(&parando, INTERVALO) {
            if let Some(c) = &atual {
                c.matar(); // H3: ao parar o serviço, encerra a cápsula que ele lançou
            }
            return;
        }
        let sessao = unsafe { WTSGetActiveConsoleSessionId() };
        if sessao == SEM_SESSAO {
            // Sem console (logoff / troca de usuário em andamento): mata a cápsula da sessão que
            // saiu antes de soltar o handle. O Drop só faz CloseHandle — sem o matar() o processo
            // ficaria órfão rodando na sessão de fundo.
            if let Some(c) = &atual {
                c.matar();
            }
            atual = None;
            sessao_atual = SEM_SESSAO;
            continue;
        }
        if sessao != sessao_atual {
            // Console mudou (troca rápida de usuário): encerra a cápsula da sessão anterior antes
            // de lançar na nova — senão vira órfã na sessão de fundo (mesmo motivo acima).
            if let Some(c) = &atual {
                c.matar();
            }
            atual = None;
            partidas.clear();
            sessao_atual = sessao;
        }
        if atual.as_ref().map(Capsula::viva).unwrap_or(false) {
            continue;
        }
        atual = None;

        let agora = Instant::now();
        partidas.retain(|t| agora.duration_since(*t) < JANELA);
        if partidas.len() >= MAX_PARTIDAS {
            transicao(&mut ultimo_log, "freio", "cápsula reiniciou demais; pausando 10 min (possível crash)");
            if dormir_ou_parar(&parando, PAUSA_APOS_FREIO) {
                if let Some(c) = &atual {
                    c.matar();
                }
                return;
            }
            partidas.clear();
            continue;
        }

        match lancar(&capsula_exe, sessao) {
            // H2(a): só a partida bem-sucedida conta no freio (nasce-e-morre em laço).
            Ok(c) => {
                partidas.push(agora);
                transicao(&mut ultimo_log, "ok", &format!("cápsula lançada (sessão {sessao})"));
                atual = Some(c);
            }
            // H2(b): sem usuário não conta e não martela o log (H2c: só na transição).
            Err(FalhaLancar::SemUsuario) => {
                transicao(&mut ultimo_log, "sem_usuario", "console sem usuário; aguardando logon")
            }
            Err(FalhaLancar::Outro(m)) => {
                transicao(&mut ultimo_log, "falha", &format!("falha ao lançar a cápsula: {m}"))
            }
        }
    }
}

/// Loga só quando o estado muda (H2c): evita encher o servico.log na tela de logon.
fn transicao(ultimo: &mut &'static str, chave: &'static str, msg: &str) {
    if *ultimo != chave {
        crate::diario::registrar(msg);
        *ultimo = chave;
    }
}

fn lancar(exe: &Path, sessao: u32) -> Result<Capsula, FalhaLancar> {
    unsafe {
        // W1: token PRIMÁRIO do usuário, já filtrado. Falhar aqui = sem usuário na sessão (H2b).
        let mut bruto: HANDLE = std::ptr::null_mut();
        if WTSQueryUserToken(sessao, &mut bruto) == 0 {
            return Err(FalhaLancar::SemUsuario);
        }
        let token = Handle(bruto);

        // W2/H1: ambiente do usuário é obrigatório; sem ele a cápsula herdaria o do SYSTEM e o
        // WebView2 abriria sem perfil (tela branca). Falhou, não lança — tenta no próximo ciclo.
        let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
        if CreateEnvironmentBlock(&mut env, token.0, 0) == 0 {
            return Err(FalhaLancar::Outro("CreateEnvironmentBlock falhou".into()));
        }

        let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        // W5: --pelo-servico faz o ligar_autostart da cápsula sair cedo (não regrava o HKCU\Run).
        let mut cmdline: Vec<u16> = format!("\"{}\" --pelo-servico", exe.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
        // H4: diretório de trabalho = pasta do exe, não o System32 do serviço.
        let dir_w: Option<Vec<u16>> = exe
            .parent()
            .map(|d| d.as_os_str().encode_wide().chain(Some(0)).collect());
        let dir_ptr = dir_w.as_ref().map(|v| v.as_ptr()).unwrap_or(std::ptr::null());
        let mut desktop: Vec<u16> = "winsta0\\default".encode_utf16().chain(Some(0)).collect();
        let mut si: STARTUPINFOW = std::mem::zeroed();
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.lpDesktop = desktop.as_mut_ptr();
        let mut pi: PROCESS_INFORMATION = std::mem::zeroed();

        let ok = CreateProcessAsUserW(
            token.0,
            exe_w.as_ptr(),
            cmdline.as_mut_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            0, // bInheritHandles = FALSE
            CREATE_UNICODE_ENVIRONMENT,
            env,
            dir_ptr,
            &si,
            &mut pi,
        );
        DestroyEnvironmentBlock(env);
        if ok == 0 {
            return Err(FalhaLancar::Outro("CreateProcessAsUser falhou".into()));
        }
        CloseHandle(pi.hThread);
        Ok(Capsula(pi.hProcess))
    }
}
