// SPDX-License-Identifier: AGPL-3.0-or-later
// ALICE-NFC-SaaS core-engine: NFC protocol platform

use axum::{
    extract::State,
    http::StatusCode,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tower_http::{cors::CorsLayer, trace::TraceLayer};
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Default)]
struct Stats {
    reads: u64,
    writes: u64,
    ndef_ops: u64,
    emulations: u64,
}

type AppState = Arc<Mutex<Stats>>;

#[derive(Deserialize)]
struct ReadRequest {
    reader_id: String,
    tag_type: Option<String>,
    timeout_ms: Option<u32>,
}

#[derive(Deserialize)]
struct WriteRequest {
    reader_id: String,
    tag_uid: String,
    data: Vec<u8>,
    offset: Option<u32>,
}

#[derive(Deserialize)]
struct NdefRequest {
    tag_uid: String,
    operation: String,
    records: Option<Vec<NdefRecord>>,
}

#[derive(Deserialize, Serialize)]
struct NdefRecord {
    tnf: u8,
    record_type: String,
    payload: String,
}

#[derive(Deserialize)]
struct EmulateRequest {
    card_type: String,
    uid: String,
    ndef_data: Option<String>,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

async fn health() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "ok",
        service: "alice-nfc-core",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn read(
    State(state): State<AppState>,
    Json(req): Json<ReadRequest>,
) -> (StatusCode, Json<Value>) {
    let read_id = Uuid::new_v4().to_string();
    let tag_type = req.tag_type.unwrap_or_else(|| "ISO14443A".to_string());
    let timeout = req.timeout_ms.unwrap_or(3000);
    {
        let mut s = state.lock().unwrap();
        s.reads += 1;
    }
    info!(read_id = %read_id, reader_id = %req.reader_id, tag_type = %tag_type, "NFC read operation");
    (
        StatusCode::OK,
        Json(json!({
            "read_id": read_id,
            "reader_id": req.reader_id,
            "tag_type": tag_type,
            "timeout_ms": timeout,
            "tag_found": true,
            "uid": "04:A3:2B:C1:DE:02:80",
            "atqa": "0x0044",
            "sak": "0x00",
            "data": [0x00u8, 0x01, 0x02, 0x03],
        })),
    )
}

async fn write(
    State(state): State<AppState>,
    Json(req): Json<WriteRequest>,
) -> (StatusCode, Json<Value>) {
    let write_id = Uuid::new_v4().to_string();
    let offset = req.offset.unwrap_or(0);
    let bytes_written = req.data.len();
    {
        let mut s = state.lock().unwrap();
        s.writes += 1;
    }
    info!(write_id = %write_id, tag_uid = %req.tag_uid, bytes = bytes_written, "NFC write operation");
    (
        StatusCode::OK,
        Json(json!({
            "write_id": write_id,
            "reader_id": req.reader_id,
            "tag_uid": req.tag_uid,
            "offset": offset,
            "bytes_written": bytes_written,
            "status": "success",
        })),
    )
}

async fn ndef(
    State(state): State<AppState>,
    Json(req): Json<NdefRequest>,
) -> (StatusCode, Json<Value>) {
    let op_id = Uuid::new_v4().to_string();
    {
        let mut s = state.lock().unwrap();
        s.ndef_ops += 1;
    }
    info!(op_id = %op_id, tag_uid = %req.tag_uid, operation = %req.operation, "NDEF operation executed");
    let record_count = req.records.as_ref().map(|r| r.len()).unwrap_or(1);
    (
        StatusCode::OK,
        Json(json!({
            "op_id": op_id,
            "tag_uid": req.tag_uid,
            "operation": req.operation,
            "record_count": record_count,
            "message_size_bytes": record_count * 32,
            "status": "success",
        })),
    )
}

async fn emulate(
    State(state): State<AppState>,
    Json(req): Json<EmulateRequest>,
) -> (StatusCode, Json<Value>) {
    let session_id = Uuid::new_v4().to_string();
    {
        let mut s = state.lock().unwrap();
        s.emulations += 1;
    }
    info!(session_id = %session_id, card_type = %req.card_type, uid = %req.uid, "NFC card emulation started");
    (
        StatusCode::OK,
        Json(json!({
            "session_id": session_id,
            "card_type": req.card_type,
            "uid": req.uid,
            "ndef_data": req.ndef_data,
            "status": "emulating",
        })),
    )
}

async fn stats(State(state): State<AppState>) -> Json<Value> {
    let s = state.lock().unwrap();
    Json(json!({
        "reads": s.reads,
        "writes": s.writes,
        "ndef_ops": s.ndef_ops,
        "emulations": s.emulations,
    }))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let state: AppState = Arc::new(Mutex::new(Stats::default()));

    let app = Router::new()
        .route("/health", get(health))
        .route("/api/v1/nfc/read", post(read))
        .route("/api/v1/nfc/write", post(write))
        .route("/api/v1/nfc/ndef", post(ndef))
        .route("/api/v1/nfc/emulate", post(emulate))
        .route("/api/v1/nfc/stats", get(stats))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8140".to_string());
    let addr = format!("0.0.0.0:{}", port);
    info!("alice-nfc-core listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}
