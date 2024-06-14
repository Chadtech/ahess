use iced::{Element, widget as w};

pub struct Button<Msg> {
    text: String,
    on_press: Option<Msg>,
}


impl<Msg> Button<Msg> {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            on_press: None,
        }
    }

    pub fn on_press(mut self, msg: Msg) -> Self {
        self.on_press = Some(msg);
        self
    }

    // pub fn to_el<'a, Theme: iced::widget::button::StyleSheet, Rr: iced_core::renderer::Renderer>(&self) -> iced::widget::Button<'a, &Option<Msg>, Theme, Rr> {
    //     w::button(w::text(&self.text))
    //         .on_press(&self.on_press)
    // }

    pub fn to_el(&self) -> Button<'_, impl Fn() + 'static + Clone> {
        let mut b = w::button(w::text(&self.text));

        if let Some(on_press) = &self.on_press {
            b.on_press(self.on_press.clone())
        };

        b
    }
}


// impl<'a, Msg> Into<Element<Msg>> for Button<'a, Msg> {
//     fn into(self) -> Element<Msg> {
//         w::button(w::text(self.text)).on_press(self.on_press).into()
//     }
// }
//
// impl<'a, Message> From<Button<'a, Message>> for Element<'_, Message>
// {
//     fn from(value: Button<'a, Message>) -> Self {
//         let as_el: Element<Message> = w::button(w::text(value.text)).on_press(value.on_press).into();
//
//         as_el
//     }
// }