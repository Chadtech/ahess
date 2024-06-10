use iced::widget::{Column, Row, Space, TextInput};
use iced::{Alignment, widget as w};
use iced::application::StyleSheet;
use crate::style as s;

pub struct Model {
    name_field: String,
}

#[derive(Debug, Clone)]
pub enum Msg {
    NameChanged(String),
}

impl Model {
    pub fn init() -> Self {
        Model {
            name_field: String::new(),
        }
    }
    pub fn view(&self) -> Column<Msg> {
        Column::with_children(vec![
            Row::with_children(vec![
                w::text("name").into(),
                TextInput::new("", self.name_field.as_str()).on_input(Msg::NameChanged).into(),
            ]).spacing(
                s::S4,
            ).align_items(Alignment::Center).into(),
            Column::with_children(vec![
                Space::with_height(iced::Length::Fill).into()
            ]).into(),
            Row::with_children(vec![
                w::button(w::text("cancel")).into(),
                w::button(w::text("create")).into(),
            ]).spacing(
                s::S4,
            ).into(),
        ])
    }


    pub fn update(&mut self, message: Msg) {
        match message {
            Msg::NameChanged(name) => {
                self.name_field = name;
            }
        }
    }
}
