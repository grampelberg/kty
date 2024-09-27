use std::{
    borrow::Borrow,
    sync::{Arc, LazyLock},
};

use eyre::Result;
use kube::Resource;
use ouroboros::self_referencing;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    text::{Line, Text},
    widgets::{Block, Borders},
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
    nav::{move_cursor, BigPosition, Movement, Shrink},
    viewport::Viewport,
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

fn to_lines(txt: &str) -> Vec<Text> {
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
                .collect::<Line>()
        })
        .map(Text::from)
        .collect()
}

#[self_referencing]
struct Formatted {
    raw: String,
    #[borrows(raw)]
    #[covariant]
    lines: Vec<Text<'this>>,
}

pub struct Yaml {
    buffer: Formatted,

    position: BigPosition,
}

impl Yaml {
    pub fn new<K>(resource: &Arc<K>) -> Self
    where
        K: Resource<DynamicType = ()> + Serialize + Send + Sync + 'static,
    {
        WIDGET_VIEWS_VEC
            .with_label_values(&[K::kind(&()).borrow(), "yaml"])
            .inc();

        let buffer = FormattedBuilder {
            raw: resource.to_yaml().expect("has yaml"),
            lines_builder: |raw| to_lines(raw),
        }
        .build();

        Self {
            buffer,
            position: BigPosition::default(),
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

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        let block = Block::default().borders(Borders::ALL);
        let inner = block.inner(area);
        let txt = self.buffer.borrow_lines();

        self.position.y = self.position.y.clamp(
            0,
            txt.len().saturating_sub(usize::from(area.height)).shrink(),
        );

        let pos = self.position;

        let result = Viewport::builder()
            .buffer(txt)
            .view(pos)
            .build()
            .draw(frame, inner);

        frame.render_widget(block, area);

        result
    }
}
