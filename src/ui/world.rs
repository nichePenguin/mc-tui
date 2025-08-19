use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ratatui::widgets::StatefulWidgetRef;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Rect, Position};

pub struct WorldWidget {
}

impl WorldWidget {
    pub fn new() -> Self {
        Self {}
    }
}

impl StatefulWidgetRef for &WorldWidget {
    type State = WorldWidgetState;
    fn render_ref(&self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        if state.map.is_none() || state.map.as_ref().unwrap().len() == 0 {
            return;
        }
        if area == state.last_area && !state.update.load(Ordering::Relaxed) {
            buf.merge(&state.last_buffer);
            return;
        }
        let map = state.map.as_ref().unwrap();
        let center = (area.width/2, area.height/2);
        let x0 = state.camera.0 as i16 - center.0 as i16;
        let y0 = state.camera.1 as i16 - center.1 as i16;
        for y in 0..area.height as i16 {
            for x in 0..area.width as i16 {
                if let Some(cell) = buf.cell_mut(Position {x: x as u16, y: y as u16}) {
                    if (x + x0) as u16 >= state.map_size.0 || (y + y0) as u16 >= state.map_size.1 {
                        cell.set_char(' ');
                        continue;
                    }
                    *cell = map[(x0+x + (y0+y)*state.map_size.0 as i16) as usize].clone();
                }
            }
        }
        state.update.store(false, Ordering::Relaxed);
        state.last_buffer = buf.clone();
        state.last_area = area;
    }
}

pub struct WorldWidgetState {
    pub map: Option<Box<[Cell]>>,
    pub map_size: (u16, u16),
    pub camera: (u16, u16),
    pub update: Arc<AtomicBool>,
    pub last_buffer: Buffer,
    pub last_area: Rect
}

impl WorldWidgetState {
    pub fn init(update: Arc<AtomicBool>) -> WorldWidgetState {
        WorldWidgetState {
            map: None,
            map_size: (0, 0),
            camera: (0, 0),
            last_buffer: Buffer::empty(Rect::ZERO),
            last_area: Rect::ZERO,
            update
        }
    }

    pub fn update(&self) {
        self.update.store(true, Ordering::Relaxed);
    }
}
