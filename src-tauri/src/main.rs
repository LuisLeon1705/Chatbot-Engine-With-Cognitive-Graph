// In release builds no separate console opens on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod appearance;
mod chat;
mod collabs;
mod language;
mod memory;
mod state;
mod timeline;

use tauri::Manager;

fn main() {
    let home = state::find_home();
    let app_state = match state::AppState::new(home.clone()) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{}", i18n::tr!(state::lang(), "app.start_failed", dir = home.display(), error = e));
            std::process::exit(1);
        }
    };

    tauri::Builder::default()
        .manage(app_state)
        .setup(|app| {
            // Development: CHATS_START="view=memory&graph=<id>&node=<n>" opens that view.
            if let (Ok(start), Some(w)) = (std::env::var("CHATS_START"), app.get_webview_window("main")) {
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    let _ = w.eval(format!("location.hash = {start:?}; location.reload();"));
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            chat::list_worlds,
            chat::list_chats,
            chat::create_chat,
            chat::get_chat,
            chat::rename_chat,
            chat::delete_chat,
            chat::set_chat_memory_mode,
            chat::set_chat_moment,
            chat::send_message,
            chat::last_context,
            chat::model_status,
            chat::stop_models,
            language::i18n_bundle,
            language::set_language,
            appearance::appearance,
            appearance::set_appearance,
            appearance::add_reference,
            appearance::describe_references,
            appearance::delete_reference,
            appearance::generate_portrait,
            memory::memory_graph,
            memory::memory_edges,
            memory::memory_update_node,
            memory::memory_create_node,
            memory::memory_delete_node,
            memory::memory_set_edge,
            memory::memory_delete_edge,
            memory::memory_probe,
            memory::memory_set_edge_validity,
            timeline::set_timeline,
            timeline::set_character_moments,
            timeline::set_world_style,
            collabs::list_collabs,
            collabs::create_collab,
            collabs::set_collab_links,
        ])
        .build(tauri::generate_context!())
        .expect("could not build the app")
        .run(|app, event| {
            // Closing the window frees the models (the Job Object also guarantees it if the
            // process dies suddenly).
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<state::AppState>();
                tauri::async_runtime::block_on(state.sup.stop_all());
            }
        });
}
