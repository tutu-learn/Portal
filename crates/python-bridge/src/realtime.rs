use pyo3::prelude::*;

#[pyfunction]
pub fn publish_realtime(
    _py: Python<'_>,
    event: String,
    message: String,
    user: Option<String>,
    room: Option<String>,
) -> PyResult<()> {
    let payload = serde_json::json!({
        "event": event,
        "message": message,
        "user": user,
    });
    let payload_str = payload.to_string();

    if let Some(pubsub) = crate::pubsub() {
        let target_room = room.as_deref().unwrap_or("global");
        pubsub.publish(target_room, &payload_str);
        if let Some(ref u) = user {
            pubsub.publish(&format!("user:{}", u), &payload_str);
        }
    } else {
        println!("[REALTIME event={} user={:?} room={:?}] {}", event, user, room, message);
    }
    Ok(())
}
