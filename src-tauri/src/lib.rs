mod api;
mod enrolar;
mod janela;

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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Abre a tarefa no navegador padrão; o escopo fica em capabilities/default.json.
        .plugin(tauri_plugin_opener::init())
        .manage(api::Api::new())
        .manage(janela::Movimento::default())
        .invoke_handler(tauri::generate_handler![
            painel_tamanho,
            painel_arrastar,
            painel_posicao,
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
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Ápice Gestor");
}
