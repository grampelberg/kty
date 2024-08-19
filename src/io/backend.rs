use std::{
    io,
    sync::{Arc, Mutex},
};

use ratatui::{
    backend::{Backend as BackendTrait, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Rect, Size},
};

/// PTY based wrapper for the crossterm backend.
///
/// Crossterm always looks for the size on the server side, this allows for
/// setting of the size from the client via resize and PTY requests.
pub struct Backend<W>
where
    W: std::io::Write + Send,
{
    crossterm: CrosstermBackend<W>,

    size: Arc<Mutex<WindowSize>>,
}

impl<W> Backend<W>
where
    W: std::io::Write + Send,
{
    pub fn with_size(writer: W) -> (Self, Arc<Mutex<WindowSize>>) {
        let size = Arc::new(Mutex::new(WindowSize {
            columns_rows: Size::default(),
            pixels: Size::default(),
        }));

        (
            Self {
                crossterm: CrosstermBackend::new(writer),
                size: size.clone(),
            },
            size,
        )
    }
}

impl<W> BackendTrait for Backend<W>
where
    W: std::io::Write + Send,
{
    fn draw<'a, I>(&mut self, items: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        self.crossterm.draw(items)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.crossterm.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.crossterm.show_cursor()
    }

    fn get_cursor(&mut self) -> io::Result<(u16, u16)> {
        self.crossterm.get_cursor()
    }

    fn set_cursor(&mut self, x: u16, y: u16) -> io::Result<()> {
        self.crossterm.set_cursor(x, y)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.crossterm.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        self.crossterm.clear_region(clear_type)
    }

    fn append_lines(&mut self, count: u16) -> io::Result<()> {
        self.crossterm.append_lines(count)
    }

    fn size(&self) -> io::Result<Rect> {
        let size = self.size.lock().unwrap();

        Ok(Rect {
            x: 0,
            y: 0,
            width: size.columns_rows.width,
            height: size.columns_rows.height,
        })
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        let size = self.size.lock().unwrap();

        Ok(*size)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.crossterm.flush()
    }
}
