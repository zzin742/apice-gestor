#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Abre a tarefa no navegador padrão; o escopo fica em capabilities/default.json.
        .plugin(tauri_plugin_opener::init())
        .run(tauri::generate_context!())
        .expect("erro ao iniciar o Ápice Gestor");
}
