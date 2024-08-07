use std::sync::{Arc, LazyLock};

use eyre::Result;
use k8s_openapi::api::core::v1::Pod;
use kube::Resource;
use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};
use tracing::info;

use super::{Dispatch, Screen, Widget};
use crate::events::{Broadcast, Event, Keypress};

pub struct Log {}

impl Log {
    pub fn new(client: kube::Client, pod: Arc<Pod>) -> Self {
        Self {}
    }
}

impl Dispatch for Log {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Event::Keypress(key) = event else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::Escape => {
                return Ok(Broadcast::Exited);
            }
            _ => {}
        }

        Ok(Broadcast::Ignored)
    }
}

impl Screen for Log {
    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let txt = "Hello, world!";
        let paragraph = Paragraph::new(txt);
        frame.render_widget(paragraph, area);
    }
}

impl Widget for Log {}
