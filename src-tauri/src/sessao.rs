//! 4/4: o serviço (SYSTEM, Sessão 0) lança a cápsula na sessão interativa do usuário, com o
//! **token do usuário** (nunca SYSTEM — webview como SYSTEM é privilégio total). Reabre se cair,
//! com freio contra laço de respawn. Laço síncrono numa thread dedicada do serviço.
//!
//! Piloto: acompanha a sessão do **console** (WTSGetActiveConsoleSessionId). Múltiplas sessões RDP
//! simultâneas e o tratamento de SESSION_CHANGE do SCM são evolução pós-piloto — o polling aqui já
//! cobre boot-com-usuário, logon, logoff e queda num PC de um usuário.
//! Só Windows; o comportamento real (token/desktop/ambiente) só a bancada confirma.
#![cfg(windows)]

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, WAIT_TIMEOUT};
use windows_sys::Win32::System::Environment::{CreateEnvironmentBlock, DestroyEnvironmentBlock};
use windows_sys::Win32::System::RemoteDesktop::{WTSGetActiveConsoleSessionId, WTSQueryUserToken};
use windows_sys::Win32::System::Threading::{
    CreateProcessAsUserW, WaitForSingleObject, CREATE_UNICODE_ENVIRONMENT, PROCESS_INFORMATION,
    STARTUPINFOW,
};

const SEM_SESSAO: u32 = 0xFFFF_FFFF;
const INTERVALO: Duration = Duration::from_secs(5);
/// Freio (W4): no máximo tantas partidas numa janela, senão pausa — crash na partida não vira laço.
const MAX_PARTIDAS: usize = 3;
const JANELA: Duration = Duration::from_secs(300);
const PAUSA_APOS_FREIO: Duration = Duration::from_secs(600);

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
}
impl Drop for Capsula {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}

pub fn lancar_loop(capsula_exe: PathBuf) {
    let mut atual: Option<Capsula> = None;
    let mut partidas: Vec<Instant> = Vec::new();
    let mut sessao_atual = SEM_SESSAO;
    loop {
        std::thread::sleep(INTERVALO);
        let sessao = unsafe { WTSGetActiveConsoleSessionId() };
        if sessao == SEM_SESSAO {
            // Logoff / sem console: esquece a cápsula (não é queda, não conta no freio).
            atual = None;
            sessao_atual = SEM_SESSAO;
            continue;
        }
        if sessao != sessao_atual {
            // Novo logon noutra sessão: recomeça do zero (uma cápsula por sessão).
            atual = None;
            partidas.clear();
            sessao_atual = sessao;
        }
        if atual.as_ref().map(Capsula::viva).unwrap_or(false) {
            continue; // ainda rodando
        }
        atual = None;

        // Freio W4: conta as partidas na janela; estourou, espera e zera.
        let agora = Instant::now();
        partidas.retain(|t| agora.duration_since(*t) < JANELA);
        if partidas.len() >= MAX_PARTIDAS {
            crate::diario::registrar("cápsula reiniciou demais; pausando 10 min (possível crash)");
            std::thread::sleep(PAUSA_APOS_FREIO);
            partidas.clear();
            continue;
        }
        partidas.push(agora);
        match lancar(&capsula_exe, sessao) {
            Ok(c) => {
                crate::diario::registrar(&format!("cápsula lançada (sessão {sessao})"));
                atual = Some(c);
            }
            Err(e) => crate::diario::registrar(&format!("falha ao lançar a cápsula: {e}")),
        }
    }
}

fn lancar(exe: &Path, sessao: u32) -> Result<Capsula, String> {
    unsafe {
        // W1: token PRIMÁRIO do usuário, já filtrado (não elevado). Usado direto, sem elevar.
        let mut bruto: HANDLE = std::ptr::null_mut();
        if WTSQueryUserToken(sessao, &mut bruto) == 0 {
            return Err("WTSQueryUserToken falhou".into());
        }
        let token = Handle(bruto);

        // W2: ambiente do usuário (%APPDATA%/%LOCALAPPDATA%), senão o WebView2 não acha o perfil.
        let mut env: *mut core::ffi::c_void = std::ptr::null_mut();
        let tem_env = CreateEnvironmentBlock(&mut env, token.0, 0) != 0;

        let exe_w: Vec<u16> = exe.as_os_str().encode_wide().chain(Some(0)).collect();
        // W5: flag própria — ligar_autostart na cápsula sai cedo e não regrava o HKCU\Run.
        let mut cmdline: Vec<u16> = format!("\"{}\" --pelo-servico", exe.display())
            .encode_utf16()
            .chain(Some(0))
            .collect();
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
            0, // W2: bInheritHandles = FALSE
            CREATE_UNICODE_ENVIRONMENT,
            env,
            std::ptr::null(),
            &si,
            &mut pi,
        );
        if tem_env {
            DestroyEnvironmentBlock(env);
        }
        if ok == 0 {
            return Err("CreateProcessAsUser falhou".into());
        }
        CloseHandle(pi.hThread);
        Ok(Capsula(pi.hProcess))
    }
}
