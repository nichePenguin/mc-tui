mod entity;
use entity::{EntityInfo, EntityType};

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::path::PathBuf;
use std::collections::{HashSet, HashMap};

use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use crossterm::event::{self, Event, KeyEventKind, KeyCode};

use crate::packets::Packet;
use crate::world::{World, BlockInfo};
use crate::log;
use crate::util::{pos_add, pos_sub, from_abs_int};
use crate::net::Connection;

pub use {
    entity::Entity
};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub struct Player {
    connection: Connection,
    pub id: usize,
    pub name: String,
    pos: (f64, f64, f64),
    pos_update: bool,
    stance: f64,
    look: (f32, f32),
    pub hp: i16,
    pub food: i16,
    pub saturation: f32,
    pub stop: bool,
    is_focused: bool,
    pub known_entities: HashSet<i32>,
    pos_update_loop: Option<tokio::task::JoinHandle<()>>
}

impl Player {
    pub async fn start(
        host: &str,
        port: i32,
        name: String
        ) -> Result<Arc<RwLock<Player>>, Box<dyn std::error::Error>>
    {
        let connection = Connection::connect_offline(host, port, name.as_str()).await?;
        // TODO obtain position and initial status from connection
        let player = Arc::new(RwLock::new(Player {
            connection,
            name: name.to_string(),
            id: NEXT_ID.fetch_add(1, Ordering::Relaxed),
            stop: false,
            pos: (0., 0., 0.),
            pos_update: false,
            stance: 0.,
            look: (0., 0.),
            hp: 0,
            food: 0,
            saturation: 0.,
            is_focused: false,
            pos_update_loop: None,
            known_entities: HashSet::new(),
        }));
        player.write().await.pos_update_loop = Some(Self::position_update_loop(Arc::clone(&player)));
        Ok(player)
    }

    pub fn world_pos(&self) -> (i32, i32, i32) {
        ((self.pos.0 - 0.5).round() as i32,
        (self.pos.1) as i32,
        (self.pos.2 - 0.5).round() as i32)
    }

    pub fn camera_pos(&self) -> (i32, i32, i32) {
        ((self.pos.0 - 0.5).round() as i32,
        (self.pos.1 + 1.4) as i32,
        (self.pos.2 - 0.5).round() as i32)
    }

    pub fn move_pos(&mut self, delta: (i32, i32, i32)) {
        self.pos = (
            self.pos.0 + delta.0 as f64,
            self.pos.1 + delta.1 as f64,
            self.pos.2 + delta.2 as f64,
        );
        self.stance += delta.1 as f64;
    }

    pub fn set_look(&mut self, look: (f32, f32)) {
        self.look = look;
    }

    fn position_update_loop(player: Arc<RwLock<Player>>) -> tokio::task::JoinHandle<()> {
        let player = Arc::clone(&player);
        tokio::task::spawn(async move {
            let mut interval = interval(Duration::from_millis(50));
            loop {
                {
                    let player = player.read().await;
                    if player.pos_update {
                        let packet = Packet::PlayerPositionAndLook {
                            x: player.pos.0,
                            stance: player.stance,
                            y: player.pos.1,
                            z: player.pos.2,
                            yaw: player.look.0,
                            pitch: player.look.1,
                            on_ground: true
                        };
                        player.connection.send(packet).await.unwrap();
                    }
                }
                interval.tick().await;
            }
        })
    }

    pub async fn tick(&mut self, ctx: &mut GlobalContext) -> bool {
        let mut inbound_buffer = vec![];
        self.connection.recv(&mut inbound_buffer).await;
        for packet in inbound_buffer.drain(..) {
            self.handle_packet(ctx, packet).await;
            if self.stop {
                if let Some(pos_update) = self.pos_update_loop.as_ref() {
                    pos_update.abort();
                }
                return true;
            }
        }
        return false;
    }

    async fn handle_packet(&mut self, ctx: &mut GlobalContext, inbound: Packet) {
        match inbound {
            Packet::SpawnPosition { x, y, z } => {
                log::info!("Spawn is at {} {} {}", x, y, z);
                self.connection.send(Packet::ClientSettings {
                    locale: "en_US".to_string(),
                    difficulty: 2,
                    view_distance: 0,
                    show_cape: true,
                    chat_flags: 8
                }).await.unwrap();
           }
            Packet::ChunkData { chunk_data } => {
                ctx.world.set_chunk(chunk_data);
                ctx.world_update = true;
            },
            Packet::ChunkDataBulk { column_data } =>  {
                ctx.world.set_chunk_bulk(&column_data);
                ctx.world_update = true;
            },
            Packet::BlockChange { x, y, z, block_type, block_meta } => {
                ctx.world.set_block(x, z, y, block_type, block_meta);
                ctx.world_update = true;
            },
            Packet::MultiBlockChange { change_data } => {
                ctx.world.set_block_multiple(&change_data);
                ctx.world_update = true;
            },
            Packet::UpdateHealth { health, food, saturation} => {
                log::info!("HP: {}, food: {}/{}", health, food, saturation);
                self.hp = health;
                self.food = food;
                self.saturation = saturation;
                if self.hp <= 0 {
                    log::info!("{} died! Respawning...", self.name);
                    self.connection.send(Packet::ClientStatuses {
                        payload: 1
                    }).await.unwrap();
                }
            },
            Packet::PlayerPositionAndLook { x, y, stance, z, yaw, pitch, on_ground } => {
                self.pos_update = true;
                self.pos = (x, stance, z);
                self.stance = stance + 0.3;
                log::info!("Is focused: {}", self.is_focused);
                if self.is_focused && let GameState::World = ctx.mode {
                    log::info!("Snapped camera to my pos");
                    ctx.set_cam(self.camera_pos());
                }
                log::info!("Forced pos to: {:?}:{}", self.pos, self.stance);
                self.connection.send(Packet::PlayerPositionAndLook {
                    x, stance: y, y: stance, z, yaw, pitch, on_ground
                }).await.unwrap();
            },
            Packet::Disconnect { reason } => {
                log::warning!("Player {} disconnected: {}", self.name, reason);
                self.stop = true;
            },
            Packet::EntityAttach {..} => {
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::SpawnObject {eid, ..} => {
                self.known_entities.insert(eid);
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::SpawnMob {eid, ..} => {
                self.known_entities.insert(eid);
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::EntityRelativeMove {..} => {
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::EntityLookAndRelativeMove {..} => {
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::EntityTeleport {..} => {
                ctx.entity_packet(inbound, self.id).await;
            },
            Packet::EntityDestroy { ids } => {
                for eid in &ids {
                    self.known_entities.remove(&eid);
                }
                ctx.entity_destroy(ids, self);
            },
            _ => {}
        }
    }
}

pub struct GlobalContext {
    pub world: World,
    // TODO split to EntityManager
    pub entities: Vec<Entity>,
    pub entity_owners: HashMap<i32, usize>,
    pub known_entities: HashSet<i32>,
    pub entities_moved: HashSet<i32>,
    pub entities_orphaned: HashSet<i32>,
    pub entities_deleted: Vec<i32>,
    pub camera: (i32, i32, i32),
    pub prev_camera: (i32, i32, i32),
    pub active_player: Option<Arc<RwLock<Player>>>,
    pub mode: GameState,
    pub players: Vec<Arc<RwLock<Player>>>,
    pub world_update: bool,
    pub camera_update: bool,
    pub entity_update: bool,
    pub block_info: Vec<BlockInfo>,
    pub entity_info: Vec<&'static EntityInfo>,
    pub tick: u64,
    pub stop: bool
}

impl GlobalContext {
    pub fn init(resources_root: PathBuf) -> Self {
        let entity_data_path = resources_root.join("entity_data.json");
        let entity_data = json::parse(&std::fs::read_to_string(entity_data_path).unwrap()[..]).unwrap();
        let block_data_path = resources_root.join("block_data.json");
        let block_data = json::parse(&std::fs::read_to_string(block_data_path).unwrap()[..]).unwrap();
        Self {
            world: World::new(),
            known_entities: HashSet::new(),
            entities: vec![],
            entity_owners: HashMap::new(),
            camera: (0, 0, 0),
            prev_camera: (0, 0, 0),
            world_update: true,
            camera_update: true,
            entity_update: true,
            entities_moved: HashSet::new(),
            entities_deleted: vec![],
            entities_orphaned: HashSet::new(),
            active_player: None,
            players: vec![],
            mode: GameState::World,
            block_info: block_data["data"]
                .members()
                .map(|block| BlockInfo { 
                    id: block["id"].as_u16().unwrap_or(std::u16::MAX),
                    is_solid: block["isSolid"].as_bool().unwrap_or(false)
                }).collect(),
            entity_info: entity_data["data"]
                .members()
                .map(|entity| {
                    let etype = match entity["type"].as_str().unwrap() {
                        "mob" => EntityType::Mob(entity::to_mob_type(entity["id"].as_u8().unwrap())),
                        "object" => EntityType::Object(entity::to_object_type(entity["id"].as_u8().unwrap())),
                        _ => panic!("Unknown type of entity: {:?}", entity["type"])
                    };
                    &*Box::leak(Box::new(EntityInfo {
                        etype,
                        id: entity["id"].as_u8().unwrap(),
                        name: entity["name"].as_str().unwrap().to_string(),
                        sprites: entity["sprites"].members().map(|s| {
                            let character = s["char"].as_str().unwrap().chars().next().unwrap();
                            let color: Vec<u8> = s["color"].members().map(|e| e.as_u8().unwrap()).collect();
                            if s.has_key("bg") {
                                let bg: Vec<u8> = s["bg"].members().map(|e| e.as_u8().unwrap()).collect();
                                (character, (color[0], color[1], color[2]), Some((bg[0], bg[1], bg[2])))
                            } else {
                                (character, (color[0], color[1], color[2]), None)
                            }
                        }).collect()
                    }))
                })
                .collect(),
            tick: 0,
            stop: false
        }
    }

    pub async fn tick(&mut self) {
        if self.tick == std::u64::MAX {
            log::info!("How did you get here?");
            self.tick = 0;
        }
        self.tick += 1;
        self.world_update = false;
        self.camera_update = false;
        self.entities_moved.clear();
        self.entities_deleted.clear();
        self.entity_update = false;

        self.entity_tick().await;

        for player in self.players.clone().iter() {
            self.entities_orphaned.clear();
            {
                let mut player = player.write().await;
                if !player.stop {
                    player.tick(self).await;
                }
            }
            for orphan in &self.entities_orphaned {
                let mut new_owner = false;
                for player in &self.players {
                    let player = player.read().await;
                    if self.known_entities.contains(&orphan) {
                        self.entity_owners.insert(*orphan, player.id);
                        new_owner = true;
                    }
                }
                if !new_owner {
                    if let Some(index) = self.entities.iter().position(|e| e.id == *orphan) {
                        self.entities_deleted.push(*orphan);
                        self.known_entities.remove(orphan);
                        self.entities.remove(index);
                        self.entity_update = true;
                    }
                }
            }
        }

        if event::poll(Duration::from_millis(1)).unwrap() {
            if let Ok(Event::Key(key)) = event::read() {
                if key.kind == KeyEventKind::Press {
                    handle_input(key.code, self).await;
                }
            }
        }
    }

    async fn entity_tick(&mut self) {
        for entity in self.entities.iter_mut() {
            entity.last_position = entity.pos;
            entity.last_movement = (0., 0., 0.);
            entity.new = false;
        }
    }

    fn detach(&mut self, eid: i32) {
        let entity_idx = self.entities.iter().position(|e| e.id == eid);
        if entity_idx.is_none() {
            return;
        }
        let entity = &mut self.entities[entity_idx.unwrap()];
        entity.parent = None;
        if let Some(vehicle_id) = entity.parent {
            if let Some(vehicle) = self.entities.iter_mut().find(|e| e.id == vehicle_id) {
                if let Some(child_idx) = vehicle.children.iter().position(|e| *e == eid) {
                    vehicle.children.remove(child_idx);
                }
            }
        }
    }

    fn attach(&mut self, eid: i32, vehicle_id: i32) {
        if let Some(entity) = self.entities.iter_mut().find(|e| e.id == eid) {
            entity.parent = Some(vehicle_id);
        }
        if let Some(vehicle) = self.entities.iter_mut().find(|e| e.id == vehicle_id) {
            vehicle.children.push(eid);
        }
    }

    async fn entity_packet(&mut self, packet: Packet, source: usize) {
        match packet {
            Packet::EntityAttach {eid, vehicle_eid} => {
                if self.entity_owners.get(&eid).map(|v| *v) != Some(source) {
                    return;
                }
                if vehicle_eid == -1 {
                    self.detach(eid);
                } else {
                    self.attach(eid, vehicle_eid);
                }
            },
            Packet::SpawnObject {eid, obj_type, x, y, z, pitch, yaw, object_data } => {
                if self.known_entities.contains(&eid) {
                    return;
                }
                self.entity_owners.insert(eid, source);
                self.known_entities.insert(eid);
                let etype = EntityType::Object(entity::to_object_type(obj_type));
                let pos = from_abs_int((x, y, z));
                let info = self.entity_info.iter().find(|info| info.etype == etype).map(|e| *e);
                self.entities.push(Entity {
                   etype,
                   new: true,
                   id: eid,
                   name: None,
                   info,
                   pos,
                   last_position: pos,
                   parent: None,
                   children: vec![],
                   last_movement: (0., 0., 0.),
                });
                self.entity_update = true;
},
            Packet::SpawnMob {eid, mob_type, x, y, z, pitch, head_pitch, yaw, dx, dy, dz, metadata} => {
                if self.known_entities.contains(&eid) {
                    return;
                }
                self.entity_owners.insert(eid, source);
                self.known_entities.insert(eid);
                let etype = EntityType::Mob(entity::to_mob_type(mob_type));
                let pos = from_abs_int((x, y, z));
                let info = self.entity_info.iter().find(|info| info.etype == etype).map(|e| *e);
                self.entities.push(Entity {
                   etype,
                   new: true,
                   id: eid,
                   name: None,
                   info,
                   pos,
                   last_position: pos,
                   parent: None,
                   children: vec![],
                   last_movement: (0., 0., 0.),
                });
                self.entity_update = true;
            },
            Packet::EntityTeleport {eid, x, y, z, yaw, pitch} => {
                self.entity_move(from_abs_int((x, y, z)), true, eid, source);
            },
            Packet::EntityLookAndRelativeMove {eid, dx, dy, dz, yaw, pitch} => {
                self.entity_move(from_abs_int((dx, dy, dz)), false, eid, source);
            },
            Packet::EntityRelativeMove {eid, dx, dy, dz} => {
                self.entity_move(from_abs_int((dx, dy, dz)), false, eid, source);
            },
            _ => {
                log::warning!("Unhandled entity packet from {}", source);
            }
        }
    }

    fn entity_move(
        &mut self,
        vector: (f64, f64, f64),
        absolute: bool,
        eid: i32,
        source: usize) 
    {
        if let Some(owner) = self.entity_owners.get(&eid) {
            if *owner != source {
                return;
            }
            let mut position = (0., 0., 0.);
            let mut children = vec![];
            if let Some(entity) = self.entities.iter_mut().find(|e| e.id == eid) {
                self.entity_update = true;
                self.entities_moved.insert(eid);
                children = entity.children.clone();
                if absolute {
                    entity.pos = vector;
                    entity.last_movement = pos_add(entity.last_movement, pos_sub(vector, entity.pos));
                } else {
                    entity.pos = pos_add(entity.pos, vector);
                    entity.last_movement = pos_add(entity.last_movement, vector);
                }
                position = entity.pos;
            } else {
                log::warning!("Received a movement event for an untracked entity: {}", eid);
            }
            for child in children {
                if let Some(owner) = self.entity_owners.get(&child) {
                    self.entity_move(position, true, child, *owner);
                }
            }
        } else {
            log::warning!("Received a movement event for entity {} without an owner from {}!", eid, source);
        }
    }

    fn entity_destroy(&mut self, ids: Vec<i32>, player: &mut Player) {
        for eid in ids {
            if !self.known_entities.contains(&eid) {
                return;
            }
            if let Some(owner) = self.entity_owners.get(&eid){
                if *owner == player.id {
                    self.entity_owners.remove(&eid);
                    self.entities_orphaned.insert(eid);
                }
            };
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
        if self.world_update || self.camera_update {
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

    pub fn get_block_info(&self, pos: (i32, i32, i32)) -> Option<&BlockInfo> {
        let block_id = self.world.get_block(pos).id;
        self.block_info.iter().find(|b| b.id == block_id)
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
        if self.active_player.is_none() {
            return;
        }
        let world_pos = self.active_player.as_ref().unwrap().read().await.world_pos();
        let mut delta = delta;
        if delta.0 != 0 || delta.2 != 0 {
            let next = pos_add(world_pos, delta);
            // if lower target block is solid, check for two above and ascend if possible
            if let Some(block) = self.get_block_info(next) && block.is_solid {
                let bottom = pos_add(next, (0, 1, 0));
                let top = pos_add(bottom, (0, 1, 0));
                if self.get_block_info(bottom).unwrap().is_solid
                   || self.get_block_info(top).unwrap().is_solid
                {
                    return
                } else {
                    delta = pos_add(delta, (0, 1, 0));
                }
            // if not, check if block below is not solid too and descent
            } else if let Some(block) = self.get_block_info(pos_add(next, (0, -1, 0))) && !block.is_solid {
                let top = pos_add(next, (0, 1, 0));
                if !self.get_block_info(top).unwrap().is_solid {
                    delta = pos_add(delta, (0, -1, 0));
                }
            }
        }

        let yaw = match (delta.0, delta.2) {
            (0, -1) => 180,
            (-1, -1) => 135,
            (-1, 0) => 90,
            (-1, 1) => 45,
            (0, 1) => 0,
            (1, 1) => -45,
            (1, 0) => -90,
            (1, -1) => -135,
            _ => 0
        };

        let look = (yaw as f32, (-55 * delta.1) as f32);
        self.move_cam(delta);
        let mut player = self.active_player.as_mut().unwrap().write().await;
        player.move_pos(delta);
        player.set_look(look);
    }
}

pub enum GameState {
    World,
    WorldLook,
    Follow,
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
