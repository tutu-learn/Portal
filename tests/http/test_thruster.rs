use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use std::path::PathBuf;
use tower::ServiceExt;

fn build_multipart_body(boundary: &str, fields: &[(&str, Option<&str>, &[u8])]) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, filename, data) in fields {
        body.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        match filename {
            Some(f) => {
                body.extend_from_slice(
                    format!(
                        "Content-Disposition: form-data; name=\"{name}\"; filename=\"{f}\"\r\n"
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(b"Content-Type: application/octet-stream\r\n\r\n");
            }
            None => {
                body.extend_from_slice(
                    format!("Content-Disposition: form-data; name=\"{name}\"\r\n\r\n").as_bytes(),
                );
            }
        }
        body.extend_from_slice(data);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

async fn login_as_administrator(app: &axum::Router) -> String {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/login")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"usr": "Administrator", "pwd": "admin"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .find(|c| c.to_str().unwrap().starts_with("sid="))
        .cloned()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

#[tokio::test]
async fn test_thruster_chunked_upload_reassembles_file() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);
    let cookie = login_as_administrator(&app).await;

    let boundary = "----testBoundary";
    let filename = "test-chunked.bin";
    let upload_id = "test-upload-1";
    let platform = "macos";
    let content = b"0123456789".repeat(50); // 500 bytes total
    let chunk_size = 150; // 4 chunks: 150, 150, 150, 50
    let chunks: Vec<&[u8]> = content.chunks(chunk_size).collect();
    let total_chunks = chunks.len();

    for (index, chunk) in chunks.iter().enumerate() {
        let body = build_multipart_body(
            boundary,
            &[
                ("platform", None, platform.as_bytes()),
                ("filename", None, filename.as_bytes()),
                ("upload_id", None, upload_id.as_bytes()),
                ("chunk_index", None, index.to_string().as_bytes()),
                ("total_chunks", None, total_chunks.to_string().as_bytes()),
                ("file", Some(filename), chunk),
            ],
        );

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/method/thruster.upload_chunk")
                    .header(
                        "content-type",
                        format!("multipart/form-data; boundary={boundary}"),
                    )
                    .header("cookie", &cookie)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert!(
            json.get("error").is_none(),
            "chunk upload returned error: {json}"
        );

        let completed = json["completed"].as_bool().unwrap_or(false);
        if index == total_chunks - 1 {
            assert!(completed, "last chunk should mark upload as completed");
        } else {
            assert!(!completed, "intermediate chunk should not be completed");
        }
    }

    let final_path = PathBuf::from("thruster").join(platform).join(filename);
    assert!(final_path.exists(), "reassembled file should exist");
    let reassembled = tokio::fs::read(&final_path).await.unwrap();
    assert_eq!(reassembled, content.as_slice());

    // Clean up.
    let _ = tokio::fs::remove_file(&final_path).await;
    let _ = tokio::fs::remove_dir_all(PathBuf::from("thruster").join(".tmp").join(upload_id)).await;
}

#[tokio::test]
async fn test_thruster_single_upload_rejects_large_files() {
    let pool = crate::common::setup_test_db().await.unwrap();
    let state = crate::common::build_app_state(pool);
    let app = http::router::create_router().with_state(state);
    let cookie = login_as_administrator(&app).await;

    let boundary = "----testBoundary";
    let filename = "too-big.bin";
    let platform = "linux";
    let oversized = vec![0u8; 50 * 1024 * 1024 + 1];

    let body = build_multipart_body(
        boundary,
        &[
            ("platform", None, platform.as_bytes()),
            ("file", Some(filename), &oversized),
        ],
    );

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/method/thruster.upload")
                .header("content-type", format!("multipart/form-data; boundary={boundary}"))
                .header("cookie", &cookie)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        json.get("error")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("upload_chunk"),
        "error should direct caller to chunked endpoint: {json}"
    );
}
