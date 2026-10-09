mod api;
mod batimento;
mod bloqueio;
mod cofre;
mod comandos;
mod desligar;
mod enrolar;
mod janela;
mod servico;

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, State, WebviewWindow, WindowEvent};

/// Muda o tamanho do painel mantendo parado o canto `ancora` da janela
/// (`inferior-direito`, padrão; `inferior-esquerdo`, `superior-direito`, `superior-esquerdo`).
/// `largura` e `altura` em px CSS (lógicos), com a folga da sombra.
#[tauri::command]
fn painel_tamanho(
    window: WebviewWindow,
    largura: f64,
    altura: f64,
    ancora: Option<String>,
) -> Result<(), String> {
    let ancora = ancora.as_deref().unwrap_or("inferior-direito");
    janela::redimensionar(&window, largura, altura, ancora).map_err(|e| e.to_string())
}

/// Começa o arraste nativo; a interface chama com o botão do mouse ainda pressionado.
#[tauri::command]
fn painel_arrastar(window: WebviewWindow) -> Result<(), String> {
    window.start_dragging().map_err(|e| e.to_string())
}

/// Janela relativa à área útil do monitor onde está, em px lógicos.
#[tauri::command]
fn painel_posicao(window: WebviewWindow) -> Result<janela::Posicao, String> {
    janela::posicao(&window).map_err(|e| e.to_string())
}

/// Sinais da interface para o heartbeat: `aberto` (o usuário abriu o cartão) ou `bom_dia`.
/// Sem retorno; a interface ignora falha.
#[tauri::command]
fn painel_evento(telemetria: State<'_, batimento::Telemetria>, tipo: String) {
    telemetria.registrar(&tipo);
}

/// `GET me/tasks`: o cartão inteiro, já resolvido pelo Hub (contrato §3).
#[tauri::command]
async fn gestor_tarefas(api: State<'_, api::Api>) -> Result<serde_json::Value, api::ErroApi> {
    api.get("me/tasks").await
}

/// `GET me/config`: flags e parâmetros que o agente obedece (contrato §4).
#[tauri::command]
async fn gestor_config(api: State<'_, api::Api>) -> Result<serde_json::Value, api::ErroApi> {
    api.get("me/config").await
}

/// Ícone na bandeja com "Voltar ao canto". O texto do menu passa pelo José Luiz antes da bancada.
fn bandeja(app: &tauri::App) -> tauri::Result<()> {
    let voltar = MenuItem::with_id(app, "voltar-ao-canto", "Voltar ao canto", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&voltar])?;
    let mut icone = TrayIconBuilder::with_id("bandeja")
        .tooltip("Ápice Gestor")
        .menu(&menu)
        .on_menu_event(|app, evento| {
            if evento.id() == "voltar-ao-canto" {
                if let Some(painel) = app.get_webview_window("main") {
                    let _ = janela::voltar_ao_canto(&painel);
                }
            }
        });
    if let Some(imagem) = app.default_window_icon() {
        icone = icone.icon(imagem.clone());
    }
    icone.build(app)?;
    Ok(())
}

/// `--enrolar <código>`: devolve o código de saída do processo.
pub fn enrolar(codigo: &str) -> i32 {
    enrolar::executar(codigo)
}

/// Modo serviço (`apice-gestor-servico.exe`): bloqueia até o SCM parar. Devolve o código de saída.
pub fn rodar_servico() -> i32 {
    match servico::executar() {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("serviço: {e}");
            1
        }
    }
}

/// Em release o executável é de janela (sem console). Se o stdout não veio redirecionado,
/// escreve no console de quem chamou (PowerShell/cmd), para o `--enrolar` aparecer na tela.
pub fn usar_console_do_pai() {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::Console::{
            AttachConsole, GetStdHandle, ATTACH_PARENT_PROCESS, STD_OUTPUT_HANDLE,
        };
        let saida = GetStdHandle(STD_OUTPUT_HANDLE);
        if saida.is_null() || saida == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// Só no Windows de verdade (build de release): nunca grava item de login no Mac de desenvolvimento.
/// Elevado, não liga: o plugin tentaria HKLM primeiro (início para todos os usuários da máquina);
/// sem elevação ele cai em HKCU, só o colaborador.
fn ligar_autostart(app: &tauri::App) {
    #[cfg(all(windows, not(debug_assertions)))]
    {
        if elevado() {
            return;
        }
        let nome = &app.package_info().name;
        // Decisão do José Luiz (09/10/2026): se a pessoa desativou em Gerenciador de Tarefas →
        // Aplicativos de inicialização, respeita. O enable() do plugin regravaria como ativado.
        if desativado_pelo_usuario(nome) {
            return;
        }
        // Gravamos nós, com aspas: o enable() do plugin grava o caminho sem aspas, e com espaço
        // ("Program Files") o Windows tentaria C:\Program.exe antes. Valor já existente (do
        // instalar.ps1 ou de antes) não é tocado.
        if let Ok(exe) = std::env::current_exe() {
            registrar_run(nome, &format!("\"{}\" --sessao", exe.display()));
        }
    }
    #[cfg(not(all(windows, not(debug_assertions))))]
    let _ = app;
}

/// O Gerenciador de Tarefas marca "desativado" em HKCU\...\Explorer\StartupApproved\Run, no valor
/// com o nome do app: 12 bytes, o primeiro ímpar (0x03/0x07) e os 8 últimos com a hora da
/// desativação. Sem o valor, nunca foi desativado.
#[cfg(all(windows, not(debug_assertions)))]
fn desativado_pelo_usuario(nome: &str) -> bool {
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_BINARY};
    let largo = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let chave = largo(r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run");
    let valor = largo(nome);
    let mut dados = [0u8; 12];
    let mut tamanho = dados.len() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            chave.as_ptr(),
            valor.as_ptr(),
            RRF_RT_REG_BINARY,
            std::ptr::null_mut(),
            dados.as_mut_ptr().cast(),
            &mut tamanho,
        )
    };
    status == 0 && marcado_desativado(&dados[..tamanho as usize])
}

/// Lê o valor binário da StartupApproved: primeiro byte ímpar (0x03/0x07) ou hora da desativação
/// nos 8 últimos bytes = desativado. Vazio = nunca foi desativado.
#[cfg_attr(not(all(windows, not(debug_assertions))), allow(dead_code))]
fn marcado_desativado(dados: &[u8]) -> bool {
    match dados.first() {
        None => false,
        Some(primeiro) => {
            primeiro & 1 == 1 || (dados.len() >= 12 && dados[4..12].iter().any(|b| *b != 0))
        }
    }
}

#[cfg(test)]
mod testes {
    use super::marcado_desativado;

    #[test]
    fn startup_approved() {
        // Gravado pelo plugin/Windows quando ativado.
        assert!(!marcado_desativado(&[0x02, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        assert!(!marcado_desativado(&[0x06, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        // Desativado no Gerenciador de Tarefas: byte ímpar e a hora (FILETIME) no fim.
        assert!(marcado_desativado(&[0x03, 0, 0, 0, 0x10, 0x8f, 0x2a, 0x1b, 0x5c, 0x3a, 0xdb, 0x01]));
        assert!(marcado_desativado(&[0x07, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]));
        // Só a hora preenchida já conta como desativado (critério do auto-launch).
        assert!(marcado_desativado(&[0x02, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 0]));
        // Sem valor.
        assert!(!marcado_desativado(&[]));
    }
}

/// Grava HKCU\...\Run\<nome> = <comando> só se o valor ainda não existir.
#[cfg(all(windows, not(debug_assertions)))]
fn registrar_run(nome: &str, comando: &str) {
    use windows_sys::Win32::System::Registry::{
        RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_ANY,
    };
    let largo = |s: &str| s.encode_utf16().chain(Some(0)).collect::<Vec<u16>>();
    let chave = largo(r"Software\Microsoft\Windows\CurrentVersion\Run");
    let valor = largo(nome);
    let existe = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            chave.as_ptr(),
            valor.as_ptr(),
            RRF_RT_ANY,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    } == 0;
    if existe {
        return;
    }
    let dados = largo(comando);
    unsafe {
        RegSetKeyValueW(
            HKEY_CURRENT_USER,
            chave.as_ptr(),
            valor.as_ptr(),
            REG_SZ,
            dados.as_ptr().cast(),
            (dados.len() * 2) as u32,
        );
    }
}

/// O processo está rodando como administrador (UAC elevado)?
#[cfg(all(windows, not(debug_assertions)))]
fn elevado() -> bool {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
    unsafe {
        let mut token = std::ptr::null_mut();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
            return false;
        }
        let mut elevacao = TOKEN_ELEVATION { TokenIsElevated: 0 };
        let mut tamanho = 0u32;
        let ok = GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevacao as *mut TOKEN_ELEVATION).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut tamanho,
        );
        CloseHandle(token);
        ok != 0 && elevacao.TokenIsElevated != 0
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Primeiro plugin, como pede o single-instance: a segunda abertura só traz a existente
        // para a frente (a não ser que o Hub tenha pausado esta máquina).
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if batimento::pausado(app) {
                return;
            }
            if let Some(painel) = app.get_webview_window("main") {
                let _ = painel.show();
                let _ = painel.set_focus();
            }
        }))
        // Inicia com a sessão do Windows (chave Run do usuário); `--sessao` marca esse início.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--sessao"]),
        ))
        // Só o esqueleto: nada dispara notificação ainda.
        .plugin(tauri_plugin_notification::init())
        // Abre a tarefa no navegador padrão; o escopo fica em capabilities/default.json.
        .plugin(tauri_plugin_opener::init())
        .manage(api::Api::new())
        .manage(janela::Movimento::default())
        .manage(batimento::Telemetria::default())
        .invoke_handler(tauri::generate_handler![
            painel_tamanho,
            painel_arrastar,
            painel_posicao,
            painel_evento,
            gestor_tarefas,
            gestor_config
        ])
        .setup(|app| {
            // A janela nasce escondida; só aparece já no canto, sem piscar no meio da tela.
            let painel = app.get_webview_window("main").expect("janela main");
            janela::posicionar_ao_abrir(&painel)?;
            painel.show()?;
            let movida = painel.clone();
            painel.on_window_event(move |evento| {
                if let WindowEvent::Moved(_) = evento {
                    janela::ao_mover(&movida);
                }
            });
            janela::vigiar_monitores(painel);
            bandeja(app)?;
            ligar_autostart(app);
            desligar::marcar_inicio(); // relógio do processo para a trava (b) das ordens
            batimento::iniciar(app.handle().clone());
            comandos::iniciar(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Ápice Gestor");
}
