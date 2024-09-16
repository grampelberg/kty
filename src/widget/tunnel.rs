use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use eyre::Result;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    Frame,
};

use super::{table, Placement, Widget};
use crate::{
    events::{Broadcast, Event},
    resources,
};

pub struct Tunnel {
    zindex: Rc<RefCell<u16>>,

    items: Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>>,
    table: table::Table<Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>>>,
}

impl Tunnel {
    pub fn new(zindex: Rc<RefCell<u16>>) -> Self {
        let items = Rc::new(RefCell::new(BTreeMap::new()));

        Self {
            items: items.clone(),
            table: table::Table::builder()
                .title("Tunnels")
                .highlight(false)
                .items(items)
                .build(),
            zindex,
        }
    }

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
    fn dispatch(&mut self, event: &Event, _: &Buffer, _: Rect) -> Result<Broadcast> {
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

        let [_, area] =
            Layout::vertical(vec![Constraint::Fill(0), Constraint::Length(self.height())])
                .areas(area);

        self.table.draw(frame, area)
    }

    fn placement(&self) -> Placement {
        super::Placement {
            horizontal: Constraint::Percentage(100),
            vertical: Constraint::Length(self.height()),
        }
    }

    fn zindex(&self) -> u16 {
        *self.zindex.borrow()
    }
}

impl table::Items for Rc<RefCell<BTreeMap<resources::Tunnel, resources::Tunnel>>> {
    type Item = resources::Tunnel;

    fn items(&self, _: Option<String>) -> Vec<resources::Tunnel> {
        self.borrow().iter().map(|(_, v)| v.clone()).collect()
    }
}
