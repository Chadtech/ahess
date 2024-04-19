use crate::ahess_error::AhessError;
use iced;
use iced::widget::{column, Column};
use iced::{widget as w, Application, Command, Element, Renderer, Theme, Color};
use crate::style as s;

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
        w::container(column![w::text(self.text.clone())]).padding(s::S4).into()
    }

    fn theme(&self) -> Theme {
        fn from_ints(r: u8, g: u8, b: u8) -> Color {
            Color::from_rgb(r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0)
        }

        Theme::custom("ahess".to_string(), iced::theme::Palette {
            background: from_ints(3, 9, 7),
            text: from_ints(176, 166, 154),
            primary: from_ints(227, 211, 75),
            success: from_ints(10, 202, 26),
            danger: from_ints(242, 29, 35),
        })
    }
}

pub fn run() -> Result<(), AhessError> {
    let mut settings = iced::Settings::default();

    // settings.fonts = vec![]

    Model::run(settings).map_err(AhessError::IcedRunError)
}
