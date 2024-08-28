use std::{
    borrow::Borrow,
    sync::{Arc, LazyLock},
};

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

use super::{Widget, WIDGET_VIEWS_VEC};
use crate::{
    events::{Broadcast, Event, Keypress},
    resources::Yaml as YamlResource,
    widget::tabs::Tab,
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

// TODO:
// - Need to cache the lines.
// - See logs for performance improvements (eg. only render visible lines).
pub struct Yaml {
    txt: String,
    length: u16,
    area: Rect,
    position: (u16, u16),
}

impl Yaml {
    pub fn new<K>(resource: &Arc<K>) -> Self
    where
        K: Resource<DynamicType = ()> + Serialize + Send + Sync + 'static,
    {
        WIDGET_VIEWS_VEC
            .with_label_values(&[K::kind(&()).borrow(), "yaml"])
            .inc();

        let txt = resource.to_yaml().unwrap();

        Self {
            #[allow(clippy::cast_possible_truncation)]
            length: LinesWithEndings::from(txt.as_str()).count() as u16,
            txt,
            area: Rect::default(),
            position: (0, 0),
        }
    }

    pub fn tab<K>(name: String, resource: Arc<K>) -> Tab
    where
        K: Resource<DynamicType = ()> + Serialize + Send + Sync + 'static,
    {
        Tab::new(name, Box::new(move || Box::new(Self::new(&resource))))
    }

    fn scroll(&mut self, key: &Keypress) {
        let (x, y) = self.position;

        let next = match key {
            Keypress::CursorUp => x.saturating_sub(1),
            Keypress::CursorDown => x.saturating_add(1),
            Keypress::Control('b') => x.saturating_sub(self.area.height),
            Keypress::Control('f') | Keypress::Printable(' ') => x.saturating_add(self.area.height),
            _ => return,
        }
        .clamp(0, self.length.saturating_sub(self.area.height + 2));

        self.position = (next, y);
    }
}

impl Widget for Yaml {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        match key {
            Keypress::CursorUp
            | Keypress::CursorDown
            | Keypress::Printable(' ')
            | Keypress::Control('b' | 'f') => self.scroll(key),
            _ => return Ok(Broadcast::Ignored),
        }

        Ok(Broadcast::Consumed)
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        self.area = area;

        let lines = to_lines(self.txt.as_str());

        frame.render_widget(Paragraph::new(lines).scroll(self.position), area);

        Ok(())
    }
}
