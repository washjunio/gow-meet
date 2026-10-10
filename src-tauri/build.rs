fn main() {
  // Declarar os comandos faz o Tauri exigir permissão para eles: só a janela
  // principal (capabilities/main.json) pode chamá-los; a flutuante, só `rename_voice`.
  tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
    tauri_build::AppManifest::new()
      .commands(&[
        "ensure_models", "start_session", "stop_session", "list_sessions",
        "rename_voice", "list_voices", "rename_saved_voice", "delete_voice", "delete_all_voices",
      ]),
  ))
  .expect("falha no tauri-build")
}
