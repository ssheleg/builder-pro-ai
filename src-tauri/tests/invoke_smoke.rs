// Invoke smoke: builds a mock Tauri app and calls the `ping` command end-to-end.
use tauri::test::{mock_builder, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{WebviewUrl, WebviewWindowBuilder};

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[test]
fn ping_returns_pong() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![ping])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .expect("failed to build webview");

    let res = tauri::test::get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: "ping".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            // `tauri://localhost` is the local-origin scheme Tauri uses on macOS/Linux
            // (Windows/Android use the `http://tauri.localhost` workaround); using the
            // wrong scheme makes `is_local_url` return false and the ACL layer reject
            // the invoke with "Plugin not found" even though no capability is missing.
            url: "tauri://localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );

    let value = res.expect("ping invoke returned an error");
    assert_eq!(value.deserialize::<String>().unwrap(), "pong");
}
