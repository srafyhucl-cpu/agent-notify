use serde::{Deserialize, Serialize};
use specta::Type;
use tauri_specta::Event;

use super::dto::{DeliveryStateDto, LoginSessionStateDto};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "snapshot.changed")]
pub struct SnapshotChangedEvent {
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "delivery.changed")]
pub struct DeliveryChangedEvent {
    pub delivery_id: String,
    pub notification_id: Option<String>,
    pub state: Option<DeliveryStateDto>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize, Type, Event)]
#[serde(rename_all = "camelCase")]
#[tauri_specta(event_name = "channel.login.changed")]
pub struct ChannelLoginChangedEvent {
    pub account_id: Option<String>,
    pub session_id: Option<String>,
    pub state: LoginSessionStateDto,
    pub message: Option<String>,
}
