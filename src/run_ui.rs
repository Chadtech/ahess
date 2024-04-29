use crate::ahess_error::AhessError;
use iced;
use iced::widget::{Column};
use iced::{widget as w, Application, Command, Element, Theme, Color, Font};
use crate::{job, style as s};
use crate::worker::Worker;

struct Model {
    text: String,
    worker: Worker,
}

struct Flags {
    worker: Worker,
}

#[derive(Debug, Clone)]
enum Msg {
    PressedPing,
    Finished,
    BeepInserted,
}

impl Application for Model {
    type Executor = iced::executor::Default;
    type Message = Msg;
    type Theme = Theme;
    type Flags = Flags;

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let model = Model {
            text: "Ahess!".to_string(),
            worker: flags.worker,
        };

        (model, Command::none())
    }

    fn title(&self) -> String {
        "Ahess".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Msg::PressedPing => {
                println!("Ping");
                Command::perform(insert_beep_job(self.worker.clone()), |result| {
                    dbg!(result);
                    Msg::BeepInserted
                })
            }
            Msg::Finished => {
                println!("Finished");
                Command::none()
            }
            Msg::BeepInserted => { Command::none() }
        }
    }

    fn view(&self) -> Element<Msg> {
        w::container(
            Column::with_children(vec![
                w::text(self.text.clone()).into(),
                w::button(
                    w::text("Ping"),
                ).on_press(Msg::PressedPing).into(),
            ])
        ).padding(s::S4).into()
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

async fn insert_beep_job(worker: Worker) -> Result<(), AhessError> {
    job::insert(&worker, job::Job::Beep).await?;

    Ok(())
}

pub async fn run() -> Result<(), AhessError> {
    let flags = Flags {
        worker: Worker::new().await?,
    };

    let mut settings = iced::Settings::with_flags(flags);

    settings.default_font = Font::with_name("Fira Code");

    Model::run(settings).map_err(AhessError::IcedRunError)
}

