use std::sync::{Arc, LazyLock};

use eyre::Result;
use kube::Resource;
use ratatui::{layout::Rect, text::Line, widgets::Paragraph, Frame};
use serde::Serialize;
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use syntect_tui::into_span;
use tracing::info;

use super::{Dispatch, Screen, Widget};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::Yaml as YamlResource,
};

static THEME: LazyLock<Theme> = LazyLock::new(|| {
    let ts = ThemeSet::load_defaults();
    let mut theme = ts.themes["base16-ocean.dark"].clone();
    theme.settings.background = Some(syntect::highlighting::Color {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    });

    theme
});

fn to_lines(txt: &str) -> Vec<Line> {
    let ps = SyntaxSet::load_defaults_newlines();
    let syntax = ps.find_syntax_by_extension("yaml").unwrap();

    let mut highlighter = HighlightLines::new(syntax, &THEME);

    // let txt = resource.to_yaml().unwrap();

    LinesWithEndings::from(txt)
        .map(|line| {
            highlighter
                .highlight_line(line, &ps)
                .unwrap()
                .into_iter()
                .filter_map(|segment| into_span(segment).ok())
                .collect()
        })
        .collect()
}

pub struct Yaml<K>
where
    K: Resource + Serialize + Send,
{
    resource: Arc<K>,
    txt: String,
    length: u16,
    area: Rect,
    position: (u16, u16),
}

impl<K> Yaml<K>
where
    K: Resource + Serialize + Send,
{
    pub fn new(resource: Arc<K>) -> Self {
        let txt = resource.to_yaml().unwrap();

        Self {
            resource,
            #[allow(clippy::cast_possible_truncation)]
            length: LinesWithEndings::from(txt.as_str()).count() as u16,
            txt,
            area: Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            position: (0, 0),
        }
    }

    fn scroll(&mut self, key: &Keypress) {
        let (x, y) = self.position;

        let next = match key {
            Keypress::CursorUp => x.saturating_sub(1),
            Keypress::CursorDown => x.saturating_add(1),
            Keypress::Printable(c) => {
                if c == " " {
                    x.saturating_add(self.area.height)
                } else {
                    return;
                }
            }
            _ => return,
        };

        self.position = (
            next.clamp(0, self.length.saturating_sub(self.area.height + 2)),
            y,
        );
    }
}

impl<K> Widget for Yaml<K> where K: Resource + Serialize + Send + Sync {}

impl<K> Dispatch for Yaml<K>
where
    K: Resource + Serialize + Send,
{
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Event::Keypress((key)) = event else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorUp | Keypress::CursorDown => self.scroll(key),
            Keypress::Printable(x) => {
                if x == " " {
                    self.scroll(key);
                }
            }
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }
}

impl<K> Screen for Yaml<K>
where
    K: Resource + Serialize + Send,
{
    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        self.area = area;

        let lines = to_lines(self.txt.as_str());

        frame.render_widget(Paragraph::new(lines).scroll(self.position), area);
    }
}
