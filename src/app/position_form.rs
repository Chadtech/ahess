use gpui::{div, prelude::*, App, Context, Entity};

use crate::{
    acoustics::{AcousticScene, Point3Meters},
    style as s,
    view::{field_group::field_group, text_input::TextInput},
};

pub(crate) struct PositionFields {
    id_prefix: String,
    x: Entity<TextInput>,
    y: Entity<TextInput>,
    z: Entity<TextInput>,
}

impl PositionFields {
    pub fn new<T: 'static>(
        id_prefix: impl Into<String>,
        position: Point3Meters,
        cx: &mut Context<T>,
    ) -> Self {
        Self {
            id_prefix: id_prefix.into(),
            x: cx.new(|cx| TextInput::new(position.x().to_string(), "0", cx)),
            y: cx.new(|cx| TextInput::new(position.y().to_string(), "0", cx)),
            z: cx.new(|cx| TextInput::new(position.z().to_string(), "0", cx)),
        }
    }

    pub fn position(&self, scene: &AcousticScene, cx: &App) -> Result<Point3Meters, String> {
        let x = parse_coordinate("voice X position", &self.x.read(cx).value())?;
        let y = parse_coordinate("voice Y position", &self.y.read(cx).value())?;
        let z = parse_coordinate("voice Z position", &self.z.read(cx).value())?;
        let position = Point3Meters::new(x, y, z).map_err(|error| error.to_string())?;
        scene
            .validate_source(position)
            .map_err(|error| error.to_string())?;
        Ok(position)
    }

    pub fn view(&self, scene: &AcousticScene) -> gpui::Div {
        let x_debug_id = format!("{}-position-x", self.id_prefix);
        let y_debug_id = format!("{}-position-y", self.id_prefix);
        let z_debug_id = format!("{}-position-z", self.id_prefix);
        let listener = scene.listener();
        let bounds = match scene.room() {
            Some(room) => format!(
                "room bounds: X 0–{}, Y 0–{}, Z 0–{}; listener: ({}, {}, {})",
                room.width(),
                room.length(),
                room.height(),
                listener.x(),
                listener.y(),
                listener.z(),
            ),
            None => format!(
                "listener: ({}, {}, {}); voices may be up to 1000 meters away",
                listener.x(),
                listener.y(),
                listener.z(),
            ),
        };

        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(
                div()
                    .text_color(s::FIELD_LABEL_TEXT)
                    .child("position (meters)"),
            )
            .child(
                div().flex().gap_4().children([
                    field_group("X · right", self.x.clone())
                        .debug_selector(move || x_debug_id.clone()),
                    field_group("Y · forward", self.y.clone())
                        .debug_selector(move || y_debug_id.clone()),
                    field_group("Z · up", self.z.clone())
                        .debug_selector(move || z_debug_id.clone()),
                ]),
            )
            .child(bounds)
    }
}

fn parse_coordinate(label: &'static str, value: &str) -> Result<f64, String> {
    value
        .trim()
        .parse::<f64>()
        .map_err(|_| format!("{label} must be a decimal number"))
}

#[cfg(test)]
mod tests {
    use super::parse_coordinate;

    #[test]
    fn position_fields_parse_decimal_coordinates() {
        assert_eq!(
            parse_coordinate("voice X position", " -2.5 ").unwrap(),
            -2.5
        );
        assert!(parse_coordinate("voice X position", "left").is_err());
    }
}
