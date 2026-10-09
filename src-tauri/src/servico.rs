//! Esqueleto do serviço do Windows (conta SYSTEM, início Automático). É o "sempre rodando" da
//! opção B: substitui o HKCU\Run. Este commit é só a casca do SCM (start/stop). Os laços
//! (heartbeat, comandos §8, bloqueio §9) e o lançamento da cápsula na sessão entram no commit da
//! troca atômica, quando o serviço passar a ler o token do cofre (cofre.rs) e a cápsula virar
//! cliente do IPC.
//!
//! NUNCA subir a webview aqui: uma webview como SYSTEM é privilégio total na máquina. A cápsula é
//! lançada no contexto do usuário da sessão (próximo commit).

/// Nome do serviço no SCM; o instalar.ps1/WiX da Esteira registra com este nome.
#[cfg_attr(not(windows), allow(dead_code))]
pub const NOME_SERVICO: &str = "ApiceGestor";

#[cfg(windows)]
pub fn executar() -> Result<(), String> {
    windows::rodar().map_err(|e| e.to_string())
}

#[cfg(not(windows))]
pub fn executar() -> Result<(), String> {
    // No Mac de desenvolvimento não há SCM; o modo serviço não roda aqui.
    Err("o modo serviço só existe no Windows".into())
}

#[cfg(windows)]
mod windows {
    use std::ffi::OsString;
    use std::sync::mpsc;
    use std::time::Duration;

    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    const TIPO: ServiceType = ServiceType::OWN_PROCESS;

    /// Bloqueia a thread até o serviço parar. Chamado pelo `main` do binário do serviço.
    pub fn rodar() -> windows_service::Result<()> {
        service_dispatcher::start(super::NOME_SERVICO, ffi_service_main)
    }

    define_windows_service!(ffi_service_main, service_main);

    fn service_main(_args: Vec<OsString>) {
        let _ = corpo();
    }

    fn corpo() -> windows_service::Result<()> {
        let (parar_tx, parar_rx) = mpsc::channel();

        let handler = move |evento| -> ServiceControlHandlerResult {
            match evento {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    let _ = parar_tx.send(());
                    ServiceControlHandlerResult::NoError
                }
                _ => ServiceControlHandlerResult::NotImplemented,
            }
        };
        let status = service_control_handler::register(super::NOME_SERVICO, handler)?;

        let rodando = |estado, aceita| ServiceStatus {
            service_type: TIPO,
            current_state: estado,
            controls_accepted: aceita,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };

        status.set_service_status(rodando(ServiceState::Running, ServiceControlAccept::STOP))?;

        // Esqueleto: por ora só espera o STOP. Os laços do serviço entram aqui no próximo commit,
        // lendo o token do cofre e servindo a cápsula pelo IPC.
        // TODO(serviço): cofre::ler → api/laços (batimento, comandos, bloqueio) + lançar a cápsula.
        let _ = parar_rx.recv();

        status.set_service_status(rodando(ServiceState::Stopped, ServiceControlAccept::empty()))?;
        Ok(())
    }
}
