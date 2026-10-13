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
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc;
    use std::sync::Arc;
    use std::time::Duration;

    use std::path::Path;

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
        crate::diario::registrar("serviço iniciando");
        let (parar_tx, parar_rx) = mpsc::channel();
        // H3: compartilhado com o lançador da cápsula. No STOP, ele encerra a cápsula que lançou,
        // senão o .exe fica preso e o próximo .msi (upgrade) tromba em arquivo em uso.
        let parando = Arc::new(AtomicBool::new(false));

        let parando_handler = parando.clone();
        let handler = move |evento| -> ServiceControlHandlerResult {
            match evento {
                ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
                ServiceControl::Stop => {
                    parando_handler.store(true, Ordering::SeqCst);
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
                // 4/4: mantém a cápsula viva na sessão do usuário (com o token DELE), fora do
                // runtime async — FFI síncrono do WTS/CreateProcessAsUser numa thread dedicada.
                let mut lancador = None;
                if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)) {
                    let parando_lanc = parando.clone();
                    lancador = Some(std::thread::spawn(move || {
                        crate::sessao::lancar_loop(dir.join("apice-gestor.exe"), parando_lanc)
                    }));
                }
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
                    None => crate::diario::registrar("sem pasta de dados da máquina; heartbeat e ordens não iniciados"),
                }
                // Bloqueia até o STOP. Ao retornar, o runtime é dropado e as tasks encerram.
                let _ = parar_rx.recv();
                // H3: espera o lançador encerrar a cápsula ANTES de o serviço reportar Stopped —
                // assim o .exe não fica em uso para o próximo .msi.
                if let Some(h) = lancador {
                    let _ = h.join();
                }
            }
            Err(e) => crate::diario::registrar(&format!("runtime async não iniciou: {e}")),
        }

        crate::diario::registrar("serviço parando");
        status.set_service_status(marcar(ServiceState::Stopped, ServiceControlAccept::empty()))?;
        Ok(())
    }

    /// Servidor IPC (named pipe). A2: uma conexão = um pedido + uma resposta, depois desconecta.
    /// Quantas conexões em atendimento ao mesmo tempo (F1): cada uma pode fazer um get ao Hub, então
    /// um cliente abrindo conexões em rajada não vira carga no Hub — acima disto responde rate_limited.
    const CONEXOES: usize = 8;

    async fn servir_ipc(api: Arc<Api>, telemetria: Arc<Telemetria>) {
        // E1/E2: cada conexão é atendida numa task com timeout; o laço nunca desiste do pipe.
        let limite = Arc::new(tokio::sync::Semaphore::new(CONEXOES));
        let mut servidor = instancia_com_backoff(true).await;
        loop {
            if servidor.connect().await.is_err() {
                servidor = instancia_com_backoff(false).await;
                continue;
            }
            // F2: atende a conexão JÁ aceita antes de criar a próxima — se a criação cair em backoff,
            // o cliente aceito não espera por isso.
            tokio::spawn(atender_conexao(servidor, api.clone(), telemetria.clone(), limite.clone()));
            servidor = instancia_com_backoff(false).await;
        }
    }

    /// Cria a Seguranca (bloco síncrono, sem await) e uma instância do servidor. A Seguranca é
    /// dropada ao retornar, antes de qualquer await — o future chamador fica Send.
    fn nova_instancia(
        primeira: bool,
    ) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeServer> {
        let seg = ipc::Seguranca::nova()?;
        ipc::instancia_servidor(&seg, primeira)
    }

    /// E2: nunca desiste do pipe. Em erro (inclusive sequestro do nome quando `primeira`), loga e
    /// tenta de novo com espera crescente (5→60 s), sem sair do laço — o SCM não recupera o IPC.
    async fn instancia_com_backoff(
        primeira: bool,
    ) -> tokio::net::windows::named_pipe::NamedPipeServer {
        let mut espera = 5u64;
        loop {
            match nova_instancia(primeira) {
                Ok(p) => return p,
                Err(e) => {
                    if primeira {
                        crate::diario::registrar(&format!("IPC: nome do pipe indisponível (sequestro?): {e}"));
                    } else {
                        crate::diario::registrar(&format!("IPC: criar instância falhou: {e}"));
                    }
                    tokio::time::sleep(Duration::from_secs(espera)).await;
                    espera = (espera * 2).min(60);
                }
            }
        }
    }

    /// Uma conexão: um pedido, uma resposta, desconecta (A2). Timeout em cada etapa (E1).
    async fn atender_conexao(
        mut pipe: tokio::net::windows::named_pipe::NamedPipeServer,
        api: Arc<Api>,
        telemetria: Arc<Telemetria>,
        limite: Arc<tokio::sync::Semaphore>,
    ) {
        const LIMITE: Duration = Duration::from_secs(5);
        // F1: sem permissão livre, recusa na hora (sem esperar) e desconecta.
        let _permissao = match limite.try_acquire() {
            Ok(p) => p,
            Err(_) => {
                let ocupado = Resposta::erro("rate_limited", "serviço ocupado");
                let _ = tokio::time::timeout(LIMITE, ipc::responder(&mut pipe, &ocupado)).await;
                return;
            }
        };
        let resposta = match tokio::time::timeout(LIMITE, ipc::ler_pedido(&mut pipe)).await {
            Ok(Leitura::Pedido(p)) => atender(p, &api, &telemetria).await,
            Ok(Leitura::Invalido) => Resposta::erro("bad_request", "pedido inválido"),
            // Vazia ou cliente calado até o timeout: só desconecta.
            _ => return,
        };
        let _ = tokio::time::timeout(LIMITE, ipc::responder(&mut pipe, &resposta)).await;
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

}
