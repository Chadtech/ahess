use gpui::{div, prelude::*, App, Context, Entity};

use crate::{
    acoustics::RectangularRoom,
    style as s,
    view::{
        dropdown::Dropdown,
        field_group::{compact_control_group, field_group},
        text_input::TextInput,
    },
};

const NO_ROOM_INDEX: usize = 0;
const RECTANGULAR_ROOM_INDEX: usize = 1;
const DEFAULT_WIDTH: &str = "8";
const DEFAULT_LENGTH: &str = "10";
const DEFAULT_HEIGHT: &str = "3";
const DEFAULT_REFLECTION_GAIN: &str = "0.25";

pub(crate) struct RoomFields {
    id_prefix: String,
    kind: Entity<Dropdown>,
    width: Entity<TextInput>,
    length: Entity<TextInput>,
    height: Entity<TextInput>,
    reflection_gain: Entity<TextInput>,
}

impl RoomFields {
    pub fn new<T: 'static>(
        id_prefix: impl Into<String>,
        kind_id: &'static str,
        room: Option<RectangularRoom>,
        cx: &mut Context<T>,
    ) -> Self {
        let id_prefix = id_prefix.into();
        let selected_kind = if room.is_some() {
            RECTANGULAR_ROOM_INDEX
        } else {
            NO_ROOM_INDEX
        };
        let width = room
            .map(|room| room.width().to_string())
            .unwrap_or_else(|| DEFAULT_WIDTH.to_string());
        let length = room
            .map(|room| room.length().to_string())
            .unwrap_or_else(|| DEFAULT_LENGTH.to_string());
        let height = room
            .map(|room| room.height().to_string())
            .unwrap_or_else(|| DEFAULT_HEIGHT.to_string());
        let reflection_gain = room
            .map(|room| room.reflection_gain().to_string())
            .unwrap_or_else(|| DEFAULT_REFLECTION_GAIN.to_string());
        Self {
            id_prefix,
            kind: cx.new(|cx| {
                Dropdown::new(kind_id, ["no room", "rectangular room"], selected_kind, cx)
            }),
            width: cx.new(|cx| TextInput::new(width, DEFAULT_WIDTH, cx)),
            length: cx.new(|cx| TextInput::new(length, DEFAULT_LENGTH, cx)),
            height: cx.new(|cx| TextInput::new(height, DEFAULT_HEIGHT, cx)),
            reflection_gain: cx
                .new(|cx| TextInput::new(reflection_gain, DEFAULT_REFLECTION_GAIN, cx)),
        }
    }

    pub fn kind(&self) -> Entity<Dropdown> {
        self.kind.clone()
    }

    pub fn is_enabled(&self, cx: &App) -> bool {
        self.kind.read(cx).selected_index() == RECTANGULAR_ROOM_INDEX
    }

    pub fn room(&self, cx: &App) -> Result<Option<RectangularRoom>, String> {
        if !self.is_enabled(cx) {
            return Ok(None);
        }

        let width = parse_decimal("room width", &self.width.read(cx).value())?;
        let length = parse_decimal("room length", &self.length.read(cx).value())?;
        let height = parse_decimal("room height", &self.height.read(cx).value())?;
        let reflection_gain =
            parse_decimal("reflection gain", &self.reflection_gain.read(cx).value())?;
        RectangularRoom::new(width, length, height, reflection_gain)
            .map(Some)
            .map_err(|error| error.to_string())
    }

    pub fn is_dirty(&self, original: Option<RectangularRoom>, cx: &App) -> bool {
        if self.is_enabled(cx) != original.is_some() {
            return true;
        }
        let Some(original) = original else {
            return false;
        };

        self.width.read(cx).value() != original.width().to_string()
            || self.length.read(cx).value() != original.length().to_string()
            || self.height.read(cx).value() != original.height().to_string()
            || self.reflection_gain.read(cx).value() != original.reflection_gain().to_string()
    }

    pub fn view(&self, enabled: bool) -> gpui::Div {
        let mut form = div()
            .flex()
            .flex_col()
            .gap_5()
            .child(div().text_color(s::TEXT_HEADER).child("room acoustics"))
            .child(compact_control_group("room", self.kind.clone()));

        if enabled {
            let width_debug_id = format!("{}-room-width", self.id_prefix);
            let length_debug_id = format!("{}-room-length", self.id_prefix);
            let height_debug_id = format!("{}-room-height", self.id_prefix);
            let reflection_debug_id = format!("{}-room-reflection-gain", self.id_prefix);
            form = form
                .child(
                    div().flex().gap_4().children([
                        field_group("width (meters)", self.width.clone())
                            .debug_selector(move || width_debug_id.clone()),
                        field_group("length (meters)", self.length.clone())
                            .debug_selector(move || length_debug_id.clone()),
                    ]),
                )
                .child(
                    div().flex().gap_4().children([
                        field_group("height (meters)", self.height.clone())
                            .debug_selector(move || height_debug_id.clone()),
                        field_group("reflection gain (0–1)", self.reflection_gain.clone())
                            .debug_selector(move || reflection_debug_id.clone()),
                    ]),
                )
                .child(
                    "changing the room recenters the listener and preserves each voice's offset",
                );
        }

        form
    }
}

fn parse_decimal(label: &'static str, value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label} must be a decimal number"))
}

#[cfg(test)]
mod tests {
    use super::parse_decimal;

    #[test]
    fn room_fields_parse_decimal_numbers() {
        assert_eq!(parse_decimal("room width", " 8.5 ").unwrap(), 8.5);
        assert!(parse_decimal("room width", "eight").is_err());
    }
}
