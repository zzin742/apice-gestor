mod api;

use tauri::{Manager, PhysicalPosition, PhysicalSize, State, WebviewWindow};

/// Tamanho do painel antes da interface medir a cápsula, em px lógicos.
const TAMANHO_INICIAL: (f64, f64) = (240.0, 82.0);

/// Redimensiona o painel mantendo o canto inferior direito encostado na área útil
/// do monitor principal (acima da barra de tarefas, onde ela estiver).
/// `largura` e `altura` em px CSS (lógicos); a folga da sombra vem incluída.
#[tauri::command]
fn painel_tamanho(window: WebviewWindow, largura: f64, altura: f64) -> Result<(), String> {
    ancorar(&window, largura, altura).map_err(|e| e.to_string())
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

fn ancorar(window: &WebviewWindow, largura: f64, altura: f64) -> tauri::Result<()> {
    let Some(monitor) = window.primary_monitor()? else {
        return window.set_size(tauri::LogicalSize::new(largura, altura));
    };
    let escala = monitor.scale_factor();
    let area = monitor.work_area();
    let w = (largura * escala).round() as i32;
    let h = (altura * escala).round() as i32;
    let x = area.position.x + area.size.width as i32 - w;
    let y = area.position.y + area.size.height as i32 - h;
    mover(window, x, y, w, h)
}

/// No Windows, posição e tamanho numa chamada só: sem o quadro em que a cápsula "pula".
#[cfg(windows)]
fn mover(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> tauri::Result<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER,
    };
    let hwnd = window.hwnd()?;
    unsafe {
        SetWindowPos(
            hwnd.0 as _,
            std::ptr::null_mut(),
            x,
            y,
            w,
            h,
            SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE,
        );
    }
    Ok(())
}

#[cfg(not(windows))]
fn mover(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) -> tauri::Result<()> {
    window.set_size(PhysicalSize::new(w as u32, h as u32))?;
    window.set_position(PhysicalPosition::new(x, y))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Abre a tarefa no navegador padrão; o escopo fica em capabilities/default.json.
        .plugin(tauri_plugin_opener::init())
        .manage(api::Api::new())
        .invoke_handler(tauri::generate_handler![painel_tamanho, gestor_tarefas, gestor_config])
        .setup(|app| {
            // A janela nasce escondida; só aparece já no canto, sem piscar no meio da tela.
            let painel = app.get_webview_window("main").expect("janela main");
            ancorar(&painel, TAMANHO_INICIAL.0, TAMANHO_INICIAL.1)?;
            painel.show()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Ápice Gestor");
}
