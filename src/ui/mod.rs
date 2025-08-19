use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use ratatui::style::Color;
use tokio::sync::RwLock;

mod world;
mod bar;
mod entity_overlay;

use bar::{BarWidgetState, BarWidgetDirection, BarWidgetMode};
use world::WorldWidgetState;
use entity_overlay::{EntityCellState, EntityOverlayState};

pub use {
    bar::BarWidget,
    world::WorldWidget,
    entity_overlay::EntityOverlayWidget
};

use crate::game::GlobalContext;
use crate::util::{in_square, world_pos};

const RENDER_RANGE: i32 = 200;
const RENDER_DEPTH: i32 = 7;

pub struct UiState {
    render_stop: AtomicBool,
    pub world_state: RwLock<WorldWidgetState>,
    pub entity_state: RwLock<EntityOverlayState>,
    pub hp_bar: RwLock<BarWidgetState>,
    pub food_bar: RwLock<BarWidgetState>,
}

impl UiState {
    pub fn init() -> Arc<Self> {
        let world_update = Arc::new(AtomicBool::new(true));
        let world_state = RwLock::new(WorldWidgetState::init(Arc::clone(&world_update)));
        let entity_state = RwLock::new(EntityOverlayState::init());

        let hp_bar= RwLock::new(BarWidgetState {
            color: Color::Rgb(255, 100, 100),
            direction: BarWidgetDirection::Horizontal,
            mode: BarWidgetMode::ValueWithMaxValue,
            value: 0,
            max_value: 20 
        });

        let food_bar = RwLock::new(BarWidgetState {
            color: Color::Rgb(52, 52, 209),
            direction: BarWidgetDirection::Horizontal,
            mode: BarWidgetMode::ValueWithMaxValue,
            value: 0,
            max_value: 20 
        });

        Arc::new(Self {
            render_stop: AtomicBool::new(false),
            entity_state,
            world_state,
            hp_bar,
            food_bar
        })
    }

    pub fn is_stop(&self) -> bool {
        self.render_stop.load(Ordering::Relaxed)
    }

    pub async fn set_food(&self, value: u16) {
        self.food_bar.write().await.value = value;
    }

    pub async fn set_hp(&self, value: u16) {
        self.hp_bar.write().await.value = value;
    }

    pub async fn update_entities(&self, ctx: &GlobalContext) {
        // Camera moved
        if ctx.camera_update {
            self.entities_camera_moved(ctx).await;
        }

        // Some entities moved
        if ctx.entity_update {
            self.entities_moved(ctx).await;
        }

        if ctx.tick % 60 == 0 {
            let mut entity_state = self.entity_state.write().await;
            for cell in &mut entity_state.cells {
                if cell.entities.len() > 1 {
                    cell.state = EntityCellState::Rolling;
                }
            }
        } else if (ctx.tick + 50) % 60 == 0 {
            let mut entity_state = self.entity_state.write().await;
            for cell in &mut entity_state.cells {
                cell.state = EntityCellState::Entity;
                cell.entity_index = (cell.entity_index + 1) % cell.entities.len()
            }
        }
    }

    async fn entities_moved(&self, ctx: &GlobalContext) {
        let mut entity_state = self.entity_state.write().await;
        let cam_depth = entity_state.camera.1;
        for entity in &ctx.entities { //TODO keep R/O references in a separate list?
            if !ctx.entities_moved.contains(&entity.id) {
                continue;
            }

            // Entity spawned in 
            if entity.new {
                let pos = entity.world_pos();
                if in_square(pos, ctx.camera, RENDER_RANGE, RENDER_DEPTH) &&
                    !entity_state.visible.contains(&entity.id) 
                {
                    entity_state.add(entity, pos, cam_depth);
                }
                continue;
            }

            let from = world_pos(entity.last_position);
            let to = entity.world_pos();

            // Entity moved for more than one block
            if from != to {
                if  in_square(from, ctx.camera, RENDER_RANGE, RENDER_DEPTH) &&
                    entity_state.visible.contains(&entity.id) 
                {
                    entity_state.remove(entity.id, from); 
                }
                if in_square(to, ctx.camera, RENDER_RANGE, RENDER_DEPTH) &&
                    !entity_state.visible.contains(&entity.id) 
                {
                    entity_state.add(entity, to, cam_depth);
                }
            }

            // Height changed
            if entity_state.visible.contains(&entity.id) && from.1 != to.1 {
                if let Some(cell) = entity_state.cells.iter_mut().find(|c| c.x == to.0 && c.z == to.2) {
                    if let Some(entity) = cell.entities.iter_mut().find(|e| e.id == entity.id){
                        entity.set_depth(to.1, cam_depth);
                    }
                }
            }
        }
    }

    async fn entities_camera_moved(&self, ctx: &GlobalContext) {
        let mut entity_state = self.entity_state.write().await;
        entity_state.camera = ctx.camera;
        let mut to_remove = vec![];
        // Remove abscent entities
        let EntityOverlayState { cells, visible, ..} = &mut *entity_state;
        for (i, cell) in cells.iter().enumerate() {
            if !in_square((cell.x, 0, cell.z), ctx.camera, RENDER_RANGE, RENDER_DEPTH) {
                for entity in &cell.entities {
                    visible.remove(&entity.id);
                }
                to_remove.push(i);
            }
        }

        entity_state.remove_cells(&mut to_remove);

        for entity in &ctx.entities {
            if entity_state.visible.contains(&entity.id) {
                continue;
            }
            let pos = world_pos(entity.last_position);
            if in_square(entity.world_pos(), ctx.camera, RENDER_RANGE, RENDER_DEPTH) {
                entity_state.add(entity, pos, ctx.camera.1);
            }
        }

        let cam_depth = entity_state.camera.1;
        for cell in entity_state.cells.iter_mut() {
            for entity in cell.entities.iter_mut() {
                entity.set_depth(entity.y, cam_depth);
            }
        }
    }

    pub async fn update_world(&self, ctx: &GlobalContext) {
        let (slice, camera) = ctx.world.get_slice_render(300, 100, &ctx).await;
        let mut world_state = self.world_state.write().await;
        world_state.map_size = (300, 100);
        world_state.map = Some(slice);
        world_state.camera = camera;
        world_state.update();
    }

    pub fn stop(&self) {
        self.render_stop.store(true, Ordering::Relaxed);
    }
}
