use std::collections::BTreeMap;

use eyre::Result;
use ratatui::{
    layout::{Constraint, Rect},
    Frame,
};

use super::{table, Placement, Renderable, StatefulWidget, Widget};
use crate::{
    events::{Broadcast, Event},
    resources::{self},
};

pub struct Tunnel {
    items: BTreeMap<resources::Tunnel, resources::Tunnel>,
    table: table::Table<BTreeMap<resources::Tunnel, resources::Tunnel>>,
}

impl Default for Tunnel {
    fn default() -> Self {
        Self {
            items: BTreeMap::new(),
            table: table::Table::builder()
                .title("Tunnels")
                .highlight(false)
                .build(),
        }
    }
}

impl Tunnel {
    pub fn height(&self) -> u16 {
        if self.items.is_empty() {
            return 0;
        }

        u16::try_from(self.items.len())
            .expect("no truncation")
            .saturating_add(2)
    }
}

impl Widget for Tunnel {
    fn dispatch(&mut self, event: &Event) -> Result<Broadcast> {
        Ok(match event {
            Event::Tunnel(Err(err)) => {
                let tun = err.tunnel.clone().into_error();

                self.items.insert(tun.clone(), tun);

                Broadcast::Ignored
            }
            Event::Tunnel(Ok(ev)) => {
                self.items.insert(ev.clone(), ev.clone());

                Broadcast::Consumed
            }
            _ => Broadcast::Ignored,
        })
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if self.items.is_empty() {
            return Ok(());
        }

        self.table.draw(frame, area, &mut self.items)
    }
}

impl Renderable for Tunnel {
    fn placement(&self) -> Placement {
        super::Placement {
            horizontal: Constraint::Percentage(100),
            vertical: Constraint::Length(self.height()),
        }
    }
}

impl table::Items for BTreeMap<resources::Tunnel, resources::Tunnel> {
    type Item = resources::Tunnel;

    fn items(&self) -> Vec<resources::Tunnel> {
        self.iter().map(|(_, v)| v.clone()).collect()
    }
}
