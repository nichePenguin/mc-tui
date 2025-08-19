use std::sync::Arc;
use std::path::PathBuf;

use tokio::sync::RwLock;
use tokio::time::Duration;
use crossterm::event::{self, Event, KeyEventKind, KeyCode};

mod entity;
mod entity_manager;
mod player;

use entity_manager::EntityManager;

use crate::packets::Packet;
use crate::world::{World, BlockInfo};
use crate::log;
use crate::util::pos_add;

pub use {
    entity::Entity,
    player::Player
};

pub enum GameState {
    World,
    WorldLook,
    Follow,
}

pub struct GlobalContext {
    pub tick: u64,
    pub stop: bool,
    pub mode: GameState,
    pub entities: EntityManager,
    pub world: World,
    pub active_player: Option<Arc<RwLock<Player>>>,
    pub players: Vec<Arc<RwLock<Player>>>,
    pub camera: (i32, i32, i32),
    pub prev_camera: (i32, i32, i32),
    pub camera_update: bool,
}

impl GlobalContext {
    pub fn init(resources_root: PathBuf) -> Self {
        Self {
            tick: 0,
            stop: false,
            mode: GameState::World,
            entities: EntityManager::init(resources_root.clone()),
            world: World::init(resources_root),
            active_player: None,
            players: vec![],
            camera: (0, 0, 0),
            prev_camera: (0, 0, 0),
            camera_update: true,
        }
    }

    pub async fn tick(&mut self) {
        if self.tick == std::u64::MAX {
            log::info!("How did you get here?");
            self.tick = 0;
        }
        self.tick += 1;
        self.world.update = false;
        self.camera_update = false;

        self.entities.tick();
        for player in self.players.clone().iter() {
            {
                let mut player = player.write().await;
                if !player.stop {
                    player.tick(self).await;
                }
            }
            self.entities.check_orphaned(&self.players).await;
        }

        if event::poll(Duration::from_millis(1)).unwrap() {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    handle_input(key.code, self).await;
                }
            }
        }
    }

    pub async fn update_render(&self, ui_state: &Arc<crate::ui::UiState>) {
        if let Some(player) = self.active_player.as_ref() {
            let (hp, food) = {
                let player = player.read().await;
                (player.hp, player.food)
            };
            ui_state.set_hp(hp as u16).await;
            ui_state.set_food(food as u16).await;
        }
        if self.world.update || self.camera_update {
            ui_state.update_world(&self).await;
        }
        ui_state.update_entities(&self).await;
    }

    pub async fn add_player(&mut self, player: Arc<RwLock<Player>>, set_active: bool) {
        self.players.push(Arc::clone(&player));
        if set_active {
            self.set_active_player(self.players.len()-1).await;
        }
    }

    pub async fn set_active_player(&mut self, index: usize) {
        if index < self.players.len() {
            if let Some(previous_player) = self.active_player.as_mut() {
                previous_player.write().await.is_focused = false;
            }
            let cam_pos = self.players[index].read().await.camera_pos();
            self.set_cam(cam_pos);
            let active_player = &self.players[index];
            active_player.write().await.is_focused = true;
            self.active_player = Some(Arc::clone(&active_player));
        } else {
            log::warning!("No player with index {} found", index);
        }
    }

    pub fn move_cam(&mut self, delta: (i32, i32, i32)) {
        self.set_cam((
            self.camera.0 + delta.0,
            self.camera.1 + delta.1,
            self.camera.2 + delta.2,
        ));
    }

    pub fn set_cam(&mut self, pos: (i32, i32, i32)) {
        self.prev_camera = self.camera;
        self.camera = pos;
        self.camera_update = true;
    }

    pub async fn move_player(&mut self, delta: (i32, i32, i32)) {
        match &self.active_player {
            None => {
                return;
            },
            Some(p) => {
                let cam_delta = p.write().await.move_by(&self.world, delta);
                self.move_cam(cam_delta);
            }
        }
    }
}

pub async fn handle_input(key: KeyCode, ctx: &mut GlobalContext) {
    match ctx.mode {
        GameState::World => handle_input_world(key, ctx).await,
        GameState::WorldLook => handle_input_world_look(key, ctx).await,
        GameState::Follow => handle_input_follow(key, ctx).await,
    }
}

async fn handle_input_follow(key: KeyCode, ctx: &mut GlobalContext) {
}

async fn handle_input_world_look(key: KeyCode, ctx: &mut GlobalContext) {
    match key {
        KeyCode::Char('q') => {
            if let Some(player) = ctx.active_player.as_ref() {
                let cam_pos = player.read().await.camera_pos();
                ctx.set_cam(cam_pos);
            }
            ctx.mode = GameState::World;
        },
        KeyCode::Char('e') => {
            let block = ctx.world.get_block(ctx.camera);
            log::info!("Examine {:?}: {:?}", ctx.camera, block);
        },
        KeyCode::Char('y') => ctx.move_cam((-1, 0, -1)),
        KeyCode::Char('u') => ctx.move_cam((1, 0, -1)),
        KeyCode::Char('b') => ctx.move_cam((-1, 0, 1)),
        KeyCode::Char('n') => ctx.move_cam((1, 0, 1)),
        KeyCode::Char('h') => ctx.move_cam((-1, 0, 0)),
        KeyCode::Char('j') => ctx.move_cam((0, 0, -1)),
        KeyCode::Char('k') => ctx.move_cam((0, 0, 1)),
        KeyCode::Char('l') => ctx.move_cam((1, 0, 0)),
        KeyCode::Char('<') => ctx.move_cam((0, 1, 0)),
        KeyCode::Char('>') => ctx.move_cam((0, -1, 0)),
        _ => {}
    }
}

async fn handle_input_world(key: KeyCode, ctx: &mut GlobalContext) {
    match key {
        KeyCode::Char('q') => {
            for player in ctx.players.iter() {
                player.read().await.connection.send(Packet::Disconnect {
                    reason: "I'm done".to_string()
                 }).await;
            }
            ctx.stop = true;
        },
        KeyCode::Char('x') => {
            ctx.mode = GameState::WorldLook;
        },
        KeyCode::Char('y') => ctx.move_player((-1, 0, -1)).await,
        KeyCode::Char('u') => ctx.move_player((1, 0, -1)).await,
        KeyCode::Char('b') => ctx.move_player((-1, 0, 1)).await,
        KeyCode::Char('n') => ctx.move_player((1, 0, 1)).await,
        KeyCode::Char('h') => ctx.move_player((-1, 0, 0)).await,
        KeyCode::Char('j') => ctx.move_player((0, 0, -1)).await,
        KeyCode::Char('k') => ctx.move_player((0, 0, 1)).await,
        KeyCode::Char('l') => ctx.move_player((1, 0, 0)).await,
        KeyCode::Char('<') => ctx.move_player((0, 1, 0)).await,
        KeyCode::Char('>') => ctx.move_player((0, -1, 0)).await,
        _ => {}
    }
}
