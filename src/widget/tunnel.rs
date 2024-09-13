use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use eyre::Result;
use ratatui::{
    layout::{Constraint, Rect},
    Frame,
};

use super::{table, Placement, Widget};
use crate::{
    events::{Broadcast, Event},
    resources,
};

pub struct Tunnel {
    items: Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>>,
    table: table::Table<Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>>>,
}

impl Default for Tunnel {
    fn default() -> Self {
        let items = Rc::new(RefCell::new(BTreeMap::new()));

        Self {
            items: items.clone(),
            table: table::Table::builder()
                .title("Tunnels")
                .highlight(false)
                .items(items)
                .build(),
        }
    }
}

impl Tunnel {
    pub fn height(&self) -> u16 {
        if self.items.borrow().is_empty() {
            return 0;
        }

        u16::try_from(self.items.borrow().len())
            .expect("no truncation")
            .saturating_add(2)
    }
}

impl Widget for Tunnel {
    fn dispatch(&mut self, event: &Event, _: Rect) -> Result<Broadcast> {
        Ok(match event {
            Event::Tunnel(Err(err)) => {
                let tun = err.tunnel.clone().into_error();

                self.items.try_borrow_mut()?.insert(tun.clone(), tun);

                Broadcast::Ignored
            }
            Event::Tunnel(Ok(ev)) => {
                self.items.try_borrow_mut()?.insert(ev.clone(), ev.clone());

                Broadcast::Consumed
            }
            _ => Broadcast::Ignored,
        })
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> Result<()> {
        if self.items.try_borrow_mut()?.is_empty() {
            return Ok(());
        }

        self.table.draw(frame, area)
    }

    fn placement(&self) -> Placement {
        super::Placement {
            horizontal: Constraint::Percentage(100),
            vertical: Constraint::Length(self.height()),
        }
    }
}

impl table::Items for Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>> {
    type Item = resources::Tunnel;

    fn items(&self, _: Option<String>) -> Vec<resources::Tunnel> {
        self.borrow().iter().map(|(_, v)| v.clone()).collect()
    }
}
