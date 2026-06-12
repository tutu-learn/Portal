use crate::handlers::{api, auth, desk, files, socketio};
use crate::websocket::ws_handler;
use crate::AppState;
use axum::{
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;

pub fn create_router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/resource/:doctype",
            get(api::get_list).post(api::insert_doc),
        )
        .route(
            "/api/resource/:doctype/:name",
            get(api::get_doc)
                .put(api::update_doc)
                .delete(api::delete_doc),
        )
        .route("/api/method/login", post(auth::login))
        .route("/api/method/logout", get(auth::logout))
        .route("/api/method/upload_file", post(files::upload_file))
        .route(
            "/api/method/frappe.desk.form.load.getdoctype",
            get(api::getdoctype_native),
        )
        .route(
            "/api/method/:method",
            get(api::call_method_get).post(api::call_method),
        )
        .route("/files/*filename", get(files::serve_public))
        .route("/private/files/*filename", get(files::serve_private))
        .route("/ws", get(ws_handler))
        .route(
            "/socket.io/",
            get(socketio::handle_get).post(socketio::handle_post),
        )
        .nest_service("/assets/frappe/dist", ServeDir::new("crates/http/assets/frappe/dist"))
        .nest_service("/assets/frappe", ServeDir::new("apps/frappe/frappe/public"))
        .nest_service("/assets", ServeDir::new("crates/http/assets"))
        .route("/login", get(desk::serve_login))
        .route("/app", get(desk::serve_desk))
        .route("/desk", get(desk::serve_desk))
        .fallback(desk::serve_desk)
        .with_state(state)
}
