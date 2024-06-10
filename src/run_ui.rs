use crate::ahess_error::AhessError;
use iced;
use iced::widget::{Column, Row};
use iced::{widget as w, Application, Command, Element, Theme, Color, Font};
use crate::{job, style as s};
use crate::ahess_result::AhessResult;
use crate::run_ui::page::{new_score, Page};
use crate::worker::Worker;

mod page;

struct Model {
    worker: Worker,
    page: Page,
}


struct Flags {
    worker: Worker,
}

#[derive(Debug, Clone)]
enum Msg {
    PressedPing,
    Finished,
    BeepInserted,
    PressedNewScore,
    NewScoreMsg(page::new_score::Msg),
}

impl Application for Model {
    type Executor = iced::executor::Default;
    type Message = Msg;
    type Theme = Theme;
    type Flags = Flags;

    fn new(flags: Self::Flags) -> (Self, Command<Self::Message>) {
        let model = Model {
            worker: flags.worker,
            page: Page::Landing,
        };

        (model, Command::none())
    }

    fn title(&self) -> String {
        "Ahess".to_string()
    }

    fn update(&mut self, message: Self::Message) -> Command<Self::Message> {
        match message {
            Msg::PressedPing => {
                Command::perform(insert_beep_job(self.worker.clone()), |_result| {
                    Msg::BeepInserted
                })
            }
            Msg::Finished => {
                Command::none()
            }
            Msg::BeepInserted => { Command::none() }
            Msg::PressedNewScore => {
                self.page = Page::NewScore(new_score::Model::init());

                Command::none()
            }
            Msg::NewScoreMsg(sub_msg) => {
                if let Page::NewScore(new_score_model) = &mut self.page {
                    new_score_model.update(sub_msg);
                }

                Command::none()
            }
        }
    }

    fn view(&self) -> Element<Msg> {
        let body: Element<Msg> = match &self.page {
            Page::Landing => {
                Column::with_children(vec![
                    Row::with_children(vec![
                        w::button(w::text("new score")).on_press(Msg::PressedNewScore).into()
                    ]).into()
                ]).into()
            }
            Page::NewScore(sub_model) => {
                let new_score_el: Element<new_score::Msg> = sub_model.view().into();
                new_score_el.map(Msg::NewScoreMsg)
            }
        };


        w::container(body).padding(s::S4).into()
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

pub async fn run() -> AhessResult<()> {
    let flags = Flags {
        worker: Worker::new().await?,
    };

    let mut settings = iced::Settings::with_flags(flags);

    settings.default_font = Font::with_name("Fira Code");

    Model::run(settings).map_err(AhessError::IcedRunError)
}

