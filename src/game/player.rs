use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::time::{interval, Duration};
use tokio::sync::RwLock;

use crate::packets::Packet;
use crate::net::Connection;
use crate::log;
use crate::util::pos_add;
use crate::world::World;

use super::{GlobalContext, GameState};

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub struct Player {
    pub connection: Connection,
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
    pub is_focused: bool,
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

    pub fn move_by(&mut self, world: &World, delta: (i32, i32, i32)) -> (i32, i32, i32) {
        let world_pos = self.world_pos();
        let mut delta = delta;
        if delta.0 != 0 || delta.2 != 0 {
            let next = pos_add(world_pos, delta);
            // if lower target block is solid, check for two above and ascend if possible
            if let Some(block) = world.get_block_info(next) && block.is_solid {
                let bottom = pos_add(next, (0, 1, 0));
                let top = pos_add(bottom, (0, 1, 0));
                if world.get_block_info(bottom).unwrap().is_solid
                   || world.get_block_info(top).unwrap().is_solid
                {
                    return (0, 0, 0);
                } else {
                    delta = pos_add(delta, (0, 1, 0));
                }
            // if not, check if block below is not solid too and descent
            } else if let Some(block) = world.get_block_info(pos_add(next, (0, -1, 0))) && !block.is_solid {
                let top = pos_add(next, (0, 1, 0));
                if !world.get_block_info(top).unwrap().is_solid {
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
        self.move_pos(delta);
        self.set_look(look);
        delta
    }

    fn move_pos(&mut self, delta: (i32, i32, i32)) {
        self.pos = (
            self.pos.0 + delta.0 as f64,
            self.pos.1 + delta.1 as f64,
            self.pos.2 + delta.2 as f64,
        );
        self.stance += delta.1 as f64;
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
                        player.connection.send(packet).await;
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
            },
            Packet::ChunkDataBulk { column_data } =>  {
                ctx.world.set_chunk_bulk(&column_data);
            },
            Packet::BlockChange { x, y, z, block_type, block_meta } => {
                ctx.world.set_block(x, z, y, block_type, block_meta);
            },
            Packet::MultiBlockChange { change_data } => {
                ctx.world.set_block_multiple(&change_data);
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
            Packet::SpawnObject {eid, ..} => {
                self.known_entities.insert(eid);
                ctx.entities.handle_packet(inbound, self.id).await;
            },
            Packet::SpawnMob {eid, ..} => {
                self.known_entities.insert(eid);
                ctx.entities.handle_packet(inbound, self.id).await;
            },
            Packet::EntityDestroy { ids } => {
                for eid in &ids {
                    self.known_entities.remove(&eid);
                }
                ctx.entities.entity_destroy(ids, self.id);
            },
            _ => {
                ctx.entities.handle_packet(inbound, self.id).await;
            }
        }
    }
}
