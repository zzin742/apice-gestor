//! Serviço do Windows (conta SYSTEM, início Automático). É o "sempre rodando" da opção B:
//! substitui o HKCU\Run. Lê o token do cofre da máquina (cofre.rs), roda os laços (heartbeat §5,
//! ordens §8, bloqueio §9) e serve a cápsula pelo IPC (named pipe). A cápsula pede; o serviço
//! responde — nunca o contrário (D1).
//!
//! NUNCA subir a webview aqui: uma webview como SYSTEM é privilégio total na máquina. A cápsula é
//! lançada no contexto do usuário da sessão (sub-peça 4/4, WTS).

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
    use std::sync::Arc;
    use std::time::Duration;

    use serde_json::Value;
    use windows_service::service::{
        ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus,
        ServiceType,
    };
    use windows_service::service_control_handler::{self, ServiceControlHandlerResult};
    use windows_service::{define_windows_service, service_dispatcher};

    use crate::api::Api;
    use crate::batimento::{self, Telemetria};
    use crate::ipc::{self, Leitura, Pedido, Resposta};
    use crate::{bloqueio, comandos, desligar};

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
        log("serviço iniciando");
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

        let marcar = |estado, aceita| ServiceStatus {
            service_type: TIPO,
            current_state: estado,
            controls_accepted: aceita,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        };

        // D5: marca "em execução" CEDO — antes do cofre e da rede. O msiexec (ServiceControl
        // Wait+Vital) espera isto; sair com erro aqui faria a instalação inteira dar rollback.
        // Sem token é o caminho NORMAL (o enrolamento vem depois): fica rodando e os laços esperam.
        status.set_service_status(marcar(ServiceState::Running, ServiceControlAccept::STOP))?;
        desligar::marcar_inicio(); // relógio do processo para a trava (b) das ordens

        // D3: runtime async próprio do serviço (não há runtime do Tauri aqui).
        match tokio::runtime::Builder::new_multi_thread().enable_all().build() {
            Ok(rt) => {
                let api = Arc::new(Api::new());
                let telemetria = Arc::new(Telemetria::default());
                rt.spawn(servir_ipc(api.clone(), telemetria.clone()));
                rt.spawn(bloqueio::rodar(api.clone()));
                // D2: dados da MÁQUINA (mesma pasta do cofre, ACL fechada), não app_local_data_dir.
                match crate::cofre::pasta_dados() {
                    Some(data_dir) => {
                        // D1: no serviço a pausa é no-op (quem esconde a janela é a cápsula).
                        let pausa: batimento::Pausa = Arc::new(|_| {});
                        rt.spawn(batimento::rodar(
                            api.clone(),
                            telemetria,
                            data_dir.clone(),
                            pausa,
                        ));
                        rt.spawn(comandos::rodar(api, data_dir));
                    }
                    None => log("sem pasta de dados da máquina; heartbeat e ordens não iniciados"),
                }
                // Bloqueia até o STOP. Ao retornar, o runtime é dropado e as tasks encerram.
                let _ = parar_rx.recv();
            }
            Err(e) => log(&format!("runtime async não iniciou: {e}")),
        }

        log("serviço parando");
        status.set_service_status(marcar(ServiceState::Stopped, ServiceControlAccept::empty()))?;
        Ok(())
    }

    /// Servidor IPC (named pipe). A2: uma conexão = um pedido + uma resposta, depois desconecta.
    async fn servir_ipc(api: Arc<Api>, telemetria: Arc<Telemetria>) {
        let mut primeira = true;
        loop {
            // A Seguranca carrega um ponteiro cru (não-Send); criamos e usamos a instância num bloco
            // síncrono e dropamos a Seguranca ANTES de qualquer await, para o future ser Send.
            let pipe = {
                let seg = match ipc::Seguranca::nova() {
                    Ok(s) => s,
                    Err(e) => {
                        log(&format!("IPC: segurança do pipe falhou: {e}"));
                        return;
                    }
                };
                match ipc::instancia_servidor(&seg, primeira) {
                    Ok(p) => p,
                    Err(e) => {
                        log(&format!("IPC: criar instância falhou: {e}"));
                        return;
                    }
                }
            };
            primeira = false;
            let mut pipe = pipe;
            if pipe.connect().await.is_err() {
                continue;
            }
            match ipc::ler_pedido(&mut pipe).await {
                Leitura::Pedido(p) => {
                    let r = atender(p, &api, &telemetria).await;
                    let _ = ipc::responder(&mut pipe, &r).await;
                }
                Leitura::Invalido => {
                    let _ = ipc::responder(
                        &mut pipe,
                        &Resposta::erro("bad_request", "pedido inválido"),
                    )
                    .await;
                }
                Leitura::Vazia => {}
            }
            // `pipe` sai de escopo aqui: desconecta. A próxima volta cria nova instância.
        }
    }

    /// Resolve um pedido da cápsula. Só leitura/telemetria: nada que desligue ou bloqueie vem daqui.
    async fn atender(pedido: Pedido, api: &Api, telemetria: &Telemetria) -> Resposta {
        match pedido {
            Pedido::Tarefas => resposta_de(api.get("me/tasks").await),
            Pedido::Config => resposta_de(api.get("me/config").await),
            Pedido::Evento { tipo } => {
                telemetria.registrar(&tipo);
                Resposta::ok(Value::Null)
            }
        }
    }

    fn resposta_de(r: Result<Value, crate::api::ErroApi>) -> Resposta {
        match r {
            Ok(dados) => Resposta::ok(dados),
            Err(e) => Resposta::erro(&e.code, &e.message),
        }
    }

    /// D6: log mínimo em %ProgramData%\ApiceGestor\servico.log — sem token, sem segredo. É o único
    /// jeito de a Esteira diagnosticar queda na bancada (stderr de um serviço vai para o nada).
    fn log(msg: &str) {
        use std::io::Write;
        let Some(pasta) = crate::cofre::pasta_dados() else { return };
        let _ = std::fs::create_dir_all(&pasta);
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(pasta.join("servico.log"))
        {
            let _ = writeln!(f, "{} v{} {}", crate::api::agora(), env!("CARGO_PKG_VERSION"), msg);
        }
    }
}
