use std::collections::HashSet;
use ratatui::style::Color;
use ratatui::widgets::WidgetRef;
use ratatui::buffer::Buffer;
use ratatui::layout::{Rect, Position};

use crate::game::Entity;

static ROLLING: [char; 4] = [
    '\\', '|', '/', '-'
];

pub struct EntityOverlayWidget<'a> {
    state: &'a EntityOverlayState,
    tick: usize
}

impl<'a> EntityOverlayWidget<'a> {
    pub fn new(state: &'a EntityOverlayState, tick: usize) -> Self {
        Self {
            state,
            tick
        }
    }
}

pub struct EntityOverlayState {
    pub cells: Vec<EntityCell>,
    pub visible: HashSet<i32>,
    pub camera: (i32, i32, i32)
}

impl EntityOverlayState {
    pub fn init() -> Self {
        Self {
            cells: vec![],
            camera: (0, 0, 0),
            visible: HashSet::new(),
        }
    }

    pub fn add(&mut self, entity: &Entity, pos: (i32, i32, i32), cam_depth: i32 ) {
        self.visible.insert(entity.id);
        let mut entity_render = EntityCellRender {
            id: entity.id,
            y: pos.1,
            frames: entity.sprites_or_default(),
        };
        if let Some(cell) = self.cells.iter_mut().find(|c| c.x == pos.0 && c.z == pos.2) {
            entity_render.set_depth(pos.1, cam_depth);
            cell.entities.push(entity_render);
            if cell.entity_index != cell.entities.len()-1 {
                cell.entity_index += 1;
            }
        } else {
            // Create a new cell
            let cell = EntityCell {
                x: pos.0,
                z: pos.2,
                state: EntityCellState::Entity,
                entity_index: 0,
                entities: vec![entity_render]
            };
            self.cells.push(cell);
        }
    }

    pub fn remove(&mut self, entity_id: i32, pos: (i32, i32, i32)) {
        self.visible.remove(&entity_id);
        if let Some(cell_index) = self.cells.iter_mut().position(|c| c.x == pos.0 && c.z == pos.2) {
            let cell = &mut self.cells[cell_index];
            if let Some(index) = cell.entities.iter().position(|e| e.id == entity_id) {
                cell.entities.remove(index);
                if cell.entity_index != 0 {
                    cell.entity_index -= 1;
                }
                if let EntityCellState::Rolling = cell.state && cell.entities.len() == 1 {
                    cell.state = EntityCellState::Entity;
                }
            }
            if cell.entities.len() == 0 {
                self.cells.remove(cell_index);
            }
        }
    }

    pub fn remove_cells(&mut self, to_remove: &mut Vec<usize>) {
        to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for index in to_remove {
            let cell = &self.cells[*index];
            for entity in &cell.entities {
                self.visible.remove(&entity.id);
            }
            self.cells.swap_remove(*index);
        }
    }
}

#[derive(Debug)]
pub struct EntityCell {
    pub x: i32,
    pub z: i32,
    pub state: EntityCellState,
    pub entity_index: usize,
    pub entities: Vec<EntityCellRender>,
}

#[derive(Debug)]
pub enum EntityCellState {
    Entity,
    Rolling
}

type EntityRender = (char, (u8, u8, u8), Option<(u8, u8, u8)>);

#[derive(Debug)]
pub struct EntityCellRender {
    pub id: i32,
    pub y: i32,
    pub frames: Vec<EntityRender>
}

impl EntityCellRender {
    pub fn set_depth(&mut self, new_depth: i32, camera_depth: i32) {
        self.y = new_depth;
        let dist = camera_depth - new_depth;

        // TODO do better
        if dist >= 1 {
            if !self.frames.iter().any(|f| f.0 == 'V' && f.1 == (0, 0, 255)) {
                self.frames.push(('V', (0, 0, 255), None));
            }
            if let Some(index) = self.frames.iter().position(|f| f.0 == 'Λ' && f.1 == (0, 0, 255)) {
                self.frames.remove(index);
            }
        } else if dist <= -1 {
            if !self.frames.iter().any(|f| f.0 == 'Λ' && f.1 == (0, 0, 255)) {
                self.frames.push(('Λ', (0, 0, 255), None));
            }
            if let Some(index) = self.frames.iter().position(|f| f.0 == 'V' && f.1 == (0, 0, 255)) {
                self.frames.remove(index);
            }
        } else {
            if let Some(index) = self.frames.iter().position(|f| f.0 == 'Λ' && f.1 == (0, 0, 255)) {
                self.frames.remove(index);
            }
            if let Some(index) = self.frames.iter().position(|f| f.0 == 'V' && f.1 == (0, 0, 255)) {
                self.frames.remove(index);
            }
        }
    }
}

impl<'a> WidgetRef for &EntityOverlayWidget<'a> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let state = self.state;
        if state.cells.is_empty() {
            return;
        }
        let center = (area.width/2, area.height/2);
        for entity in state.cells.iter() {
            let x = entity.x + center.0 as i32 - state.camera.0;
            let y = entity.z + center.1 as i32 - state.camera.2;
            if x < 0 || x > area.width as i32 || y < 0 || y > area.height as i32 {
                continue;
            }
            if let Some(cell) = buf.cell_mut(Position {x: x as u16, y: y as u16}) {
                match entity.state {
                    EntityCellState::Rolling => {
                        cell.set_char(ROLLING[(self.tick % (ROLLING.len() * 4)) / 4]);
                        cell.set_fg(Color::Rgb(142, 142, 0));
                    },
                    EntityCellState::Entity => {
                        let to_draw = &entity.entities[entity.entity_index];
                        let entity_frame = ((self.tick % 120) as f64 / (120 as f64 / to_draw.frames.len() as f64)) as usize;
                        let entity_render = &to_draw.frames[entity_frame as usize];
                        cell.set_char(entity_render.0);
                        let color = entity_render.1;
                        cell.set_fg(Color::Rgb(color.0, color.1, color.2));
                        if let Some(color) = entity_render.2 {
                            cell.set_bg(Color::Rgb(color.0, color.1, color.2));
                        }
                    }
                }
            }
        }
    }

}
