use crate::app_state::AppState;
use log::info;
use tauri::http::{header, Response, StatusCode};
use tauri::{AppHandle, Manager};

pub fn handle_carambola_protocol(
    app: &AppHandle,
    request: &tauri::http::Request<Vec<u8>>,
) -> Response<Vec<u8>> {
    let uri = request.uri().to_string();
    info!("Handling carambola protocol request: {}", uri);

    if uri.contains("/frame/") {
        if let Some(state) = app.try_state::<AppState>() {
            if let Some(frame) = state.get_frame() {
                return Response::builder()
                    .status(StatusCode::OK)
                    .header(header::CONTENT_TYPE, "image/png")
                    .header(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate")
                    .body(frame.png_bytes.clone())
                    .unwrap();
            }
        }
    }

    // Return empty 404 response
    Response::builder()
        .status(StatusCode::NOT_FOUND)
        .body(Vec::new())
        .unwrap()
}
