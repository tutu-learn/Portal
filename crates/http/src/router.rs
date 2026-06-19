use crate::handlers::{api, auth, desk, files, permissions, socketio};
use crate::websocket::ws_handler;
use crate::AppState;
use axum::{
    response::Redirect,
    routing::{get, post},
    Router,
};
use tower_http::services::ServeDir;

/// Create the base router parameterized with [`AppState`].
///
/// The caller applies state via `.with_state(state)` after all apps have
/// contributed their routes.
pub fn create_router() -> Router<AppState> {
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
        .route("/api/method/logout", post(auth::logout))
        .route("/api/method/upload_file", post(files::upload_file))
        .route(
            "/api/method/frappe.desk.form.load.getdoctype",
            get(api::getdoctype_native),
        )
        .route(
            "/api/method/frappe.desk.desk_page.getpage",
            get(api::getpage).post(api::getpage_post),
        )
        .route(
            "/api/method/frappe.core.page.permission_manager.permission_manager.get_roles_and_doctypes",
            get(permissions::get_roles_and_doctypes_get).post(permissions::get_roles_and_doctypes_post),
        )
        .route(
            "/api/method/frappe.core.page.permission_manager.permission_manager.get_permissions",
            get(permissions::get_permissions_get).post(permissions::get_permissions_post),
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
        .nest_service("/assets/frappe/node_modules", ServeDir::new("apps/frappe/node_modules"))
        .nest_service("/assets/frappe", ServeDir::new("apps/frappe/frappe/public"))
        .nest_service("/assets", ServeDir::new("crates/http/assets"))
        .route("/login", get(desk::serve_login))
        // The bundled Frappe Desk JS only strips the /desk prefix. Redirect
        // /app so the SPA sees a URL it can route correctly.
        .route("/app", get(|| async { Redirect::temporary("/desk") }))
        .route("/desk", get(desk::serve_desk))
        .fallback(desk::serve_desk)
}
