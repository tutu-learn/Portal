use axum::{
    async_trait,
    body::Bytes,
    extract::{FromRequest, Request},
    http::header::CONTENT_TYPE,
    response::{IntoResponse, Response},
};
use serde_json::Value;

/// Extractor that accepts both `application/json` and
/// `application/x-www-form-urlencoded` request bodies, converting either to
/// a `serde_json::Value`. Frappe's JS client uses form-encoding for POST
/// requests to `/api/method/*`.
pub struct AnyBody(pub Value);

#[async_trait]
impl<S> FromRequest<S> for AnyBody
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let content_type = req
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();

        let bytes = Bytes::from_request(req, state)
            .await
            .map_err(|e| e.into_response())?;

        if content_type.contains("application/json") {
            let value: Value =
                serde_json::from_slice(&bytes).unwrap_or(Value::Object(Default::default()));
            Ok(AnyBody(value))
        } else if content_type.contains("application/x-www-form-urlencoded") {
            let pairs: Vec<(String, String)> =
                serde_urlencoded::from_bytes(&bytes).unwrap_or_default();
            let mut map = serde_json::Map::new();
            for (k, v) in pairs {
                // Frappe sometimes sends nested JSON strings in form fields (e.g. `args`)
                let json_val = serde_json::from_str(&v).unwrap_or(Value::String(v));
                map.insert(k, json_val);
            }
            Ok(AnyBody(Value::Object(map)))
        } else {
            // Fallback: try JSON, otherwise empty object
            let value: Value =
                serde_json::from_slice(&bytes).unwrap_or(Value::Object(Default::default()));
            Ok(AnyBody(value))
        }
    }
}
