use iced::widget::{Column, Row, Space, TextInput};
use iced::{Alignment, widget as w};
use crate::run_ui::view::button::Button;
use crate::style as s;

pub struct Model {
    name_field: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    NameChanged(String),
    ClickedCancel,
}

impl Model {
    pub fn init() -> Self {
        Model {
            name_field: String::new(),
        }
    }
    pub fn view(&self) -> Column<Msg> {
        let name_row = Row::new().push(
            w::text("name")
        ).push(
            TextInput::new("", self.name_field.as_str()).on_input(Msg::NameChanged)
        ).spacing(
            s::S4,
        ).align_items(Alignment::Center);

        let submit_row = Row::new().push(
            Button::new("cancel").on_press(Msg::ClickedCancel).to_el().into()
        ).push(
            w::button(w::text("create")).into()
        ).spacing(
            s::S4,
        );


        Column::new()
            .push(name_row)
            .push(Space::with_height(iced::Length::Fill))
            .push(submit_row)
    }


    pub fn update(&mut self, message: Msg) {
        match message {
            Msg::NameChanged(name) => {
                self.name_field = name;
            }
            Msg::ClickedCancel => {}
        }
    }
}
