use crate::ahess_error::AhessError;
use iced;
use iced::widget::{column, Column};
use iced::{widget as w, Application, Command, Element, Renderer, Theme};

struct Model {
    text: String,
}

#[derive(Debug, Clone)]
enum Msg {
    Msg,
}

impl Application for Model {
    type Executor = iced::executor::Default;
    type Message = Msg;
    type Theme = Theme;
    type Flags = ();

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let model = Model {
            text: "AHESS!".to_string(),
        };

        (model, Command::none())
    }

    fn title(&self) -> String {
        "Ahess!!r".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Msg::Msg => Command::none(),
        }
    }

    fn view(&self) -> Element<Msg> {
        w::container(column![w::text(self.text.clone())]).into()
    }
}

pub fn run() -> Result<(), AhessError> {
    Model::run(iced::Settings::default());
    Ok(())
}
