use std::collections::{HashSet, HashMap};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::entity::{
    Entity, EntityInfo, EntityType,
    to_mob_type, to_object_type
};
use super::player::Player;

use crate::log;
use crate::util::{pos_add, pos_sub, from_abs_int};
use crate::packets::Packet;

pub struct EntityManager {
    pub update: bool,
    pub entities: Vec<Entity>,
    ids: HashSet<i32>,
    ownership: HashMap<i32, usize>,
    pub moved: HashSet<i32>,
    orphaned: HashSet<i32>,
    deleted: Vec<i32>,
    info: Vec<&'static EntityInfo>,
}

impl EntityManager {
    pub fn init(resources_root: std::path::PathBuf) -> Self {
        Self {
            entities: vec![],
            ownership: HashMap::new(),
            ids: HashSet::new(),
            moved: HashSet::new(),
            deleted: vec![],
            orphaned: HashSet::new(),
            update: true,
            info: parse_info(resources_root)
        }
    }

    pub fn tick(&mut self) {
        self.moved.clear();
        self.deleted.clear();
        self.update = false;

        for entity in self.entities.iter_mut() {
            entity.last_position = entity.pos;
            entity.last_movement = (0., 0., 0.);
            entity.new = false;
        }
    }

    pub async fn check_orphaned(&mut self, players: &Vec<Arc<RwLock<Player>>>) {
        for orphan in &self.orphaned {
                let mut new_owner = false;
                for player in players {
                    let player = player.read().await;
                    if player.known_entities.contains(&orphan) {
                        self.ownership.insert(*orphan, player.id);
                        new_owner = true;
                    }
                }
                if !new_owner {
                    if let Some(index) = self.entities.iter().position(|e| e.id == *orphan) {
                        self.deleted.push(*orphan);
                        self.ids.remove(orphan);
                        self.entities.remove(index);
                        self.update = true;
                    }
                }
            }
            self.orphaned.clear();

    }

    pub async fn handle_packet(&mut self, packet: Packet, source: usize) {
        match packet {
            Packet::EntityAttach {eid, vehicle_eid} => {
                if self.ownership.get(&eid).map(|v| *v) != Some(source) {
                    return;
                }
                if vehicle_eid == -1 {
                    self.detach(eid);
                } else {
                    self.attach(eid, vehicle_eid);
                }
            },
            Packet::SpawnObject {eid, obj_type, x, y, z, pitch, yaw, object_data } => {
                if self.ids.contains(&eid) {
                    return;
                }
                self.ownership.insert(eid, source);
                self.ids.insert(eid);
                let etype = EntityType::Object(to_object_type(obj_type));
                let pos = from_abs_int((x, y, z));
                let info = self.info.iter().find(|info| info.etype == etype).map(|e| *e);
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
                self.update = true;
            },
            Packet::SpawnMob {eid, mob_type, x, y, z, pitch, head_pitch, yaw, dx, dy, dz, metadata} => {
                if self.ids.contains(&eid) {
                    return;
                }
                self.ownership.insert(eid, source);
                self.ids.insert(eid);
                let etype = EntityType::Mob(to_mob_type(mob_type));
                let pos = from_abs_int((x, y, z));
                let info = self.info.iter().find(|info| info.etype == etype).map(|e| *e);
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
                self.update = true;
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
            _ => {}
        }
    }

    fn entity_move(
        &mut self,
        vector: (f64, f64, f64),
        absolute: bool,
        eid: i32,
        source: usize) 
    {
        if let Some(owner) = self.ownership.get(&eid) {
            if *owner != source {
                return;
            }
            let mut position = (0., 0., 0.);
            let mut children = vec![];
            if let Some(entity) = self.entities.iter_mut().find(|e| e.id == eid) {
                self.update= true;
                self.moved.insert(eid);
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
                if let Some(owner) = self.ownership.get(&child) {
                    self.entity_move(position, true, child, *owner);
                }
            }
        } else {
            log::warning!("Received a movement event for entity {} without an owner from {}!", eid, source);
        }
    }

    pub fn entity_destroy(&mut self, ids: Vec<i32>, source: usize) {
        for eid in ids {
            if !self.ids.contains(&eid) {
                return;
            }
            if let Some(owner) = self.ownership.get(&eid){
                if *owner == source {
                    self.ownership.remove(&eid);
                    self.orphaned.insert(eid);
                }
            };
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
}

fn parse_info(resources_root: std::path::PathBuf) -> Vec<&'static EntityInfo> {
    let entity_data_path = resources_root.join("entity_data.json");
    let entity_data = json::parse(&std::fs::read_to_string(entity_data_path).unwrap()[..]).unwrap();
    entity_data["data"]
        .members()
        .map(|entity| {
            let etype = match entity["type"].as_str().unwrap() {
                "mob" => EntityType::Mob(to_mob_type(entity["id"].as_u8().unwrap())),
                "object" => EntityType::Object(to_object_type(entity["id"].as_u8().unwrap())),
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
        .collect()
}
