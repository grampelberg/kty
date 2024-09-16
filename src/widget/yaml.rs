use std::{
    borrow::Borrow,
    sync::{Arc, LazyLock},
};

use eyre::Result;
use kube::Resource;
use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use serde::Serialize;
use syntect::{
    easy::HighlightLines,
    highlighting::{Theme, ThemeSet},
    parsing::SyntaxSet,
    util::LinesWithEndings,
};
use syntect_tui::into_span;

use super::{
    nav::{move_cursor, Movement},
    Widget, WIDGET_VIEWS_VEC,
};
use crate::{
    events::{Broadcast, Event},
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
    position: Position,
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
            txt,
            position: Position::default(),
        }
    }

    pub fn tab<K>(name: String, resource: Arc<K>) -> Tab
    where
        K: Resource<DynamicType = ()> + Serialize + Send + Sync + 'static,
    {
        Tab::builder()
            .name(name)
            .constructor(Box::new(move || Self::new(&resource).boxed().into()))
            .build()
    }
}

impl Widget for Yaml {
    fn dispatch(&mut self, event: &Event, _: &Buffer, area: Rect) -> Result<Broadcast> {
        let Some(key) = event.key() else {
            return Ok(Broadcast::Ignored);
        };

        if let Some(Movement::Y(y)) = move_cursor(key, area) {
            self.position.y = self.position.y.saturating_add_signed(y);

            return Ok(Broadcast::Consumed);
        }

        Ok(Broadcast::Ignored)
    }

    #[allow(clippy::cast_possible_truncation)]
    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let lines = to_lines(self.txt.as_str());

        self.position.y = self
            .position
            .y
            .clamp(0, (lines.len() as u16).saturating_sub(area.height));

        frame.render_widget(
            Paragraph::new(lines)
                .scroll((self.position.y, self.position.x))
                .block(Block::default().borders(Borders::ALL)),
            area,
        );

        Ok(())
    }
}
