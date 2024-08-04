use ratatui::{layout::Constraint, widgets::Row};

pub trait TableRow<'a> {
    fn constraints() -> Vec<Constraint>;

    fn row(&self) -> Row;
    fn header() -> Row<'a>;
}
