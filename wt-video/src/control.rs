use keycode::KeyMappingCode;
use serde::{Deserialize, Serialize};

/// The toplevel frame of the input stream, which represents some UI input.
#[derive(Debug, Serialize, Deserialize)]
pub enum ControlEvent {
    MouseEvent(MouseEvent),
    KeyboardEevnt(KeyboardEvent),
}

/// Values are [0,1] within the video feed.
#[derive(Debug, Serialize, Deserialize)]
pub struct CursorPos {
    x: f32,
    y: f32,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum MouseEvent {
    LeftClick(CursorPos),
    Move(CursorPos),
    Unfocus,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum KeyboardEvent {
    KeyDown(KeyMappingCode),
    KeyUp(KeyMappingCode),
}
