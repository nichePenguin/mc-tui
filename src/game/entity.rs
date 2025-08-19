pub struct Entity {
    pub id: i32,
    pub new: bool,
    pub etype: EntityType,
    pub name: Option<String>,
    pub pos: (f64, f64, f64),
    pub last_movement: (f64, f64, f64),
    pub last_position: (f64, f64, f64),
    pub info: Option<&'static EntityInfo>,
    pub parent: Option<i32>,
    pub children: Vec<i32>
}

impl Entity {
    pub fn world_pos(&self) -> (i32, i32, i32) {
        crate::util::world_pos(self.pos)
    }

    pub fn sprites_or_default(&self) -> Vec<EntityRender> {
        if let Some(info) = self.info {
            info.sprites.clone()
        } else {
            vec![
                ('?', (255, 0, 255), None),
                ('!', (255, 0, 255), None),
            ]
        }

    }
}

pub struct EntityInfo {
    pub etype: EntityType,
    pub id: u8,
    pub name: String,
    pub sprites: Vec<EntityRender>
}

type EntityRender = (char, (u8, u8, u8), Option<(u8, u8, u8)>);

#[derive(Debug, Eq, PartialEq)]
pub enum EntityType {
    Mob(MobType),
    Object(ObjectType),
}

#[derive(Debug, Eq, PartialEq)]
pub enum MobType {
    Creeper,
    Skeleton,
    Spider,
    GiantZombie,
    Zombie,
    Slime,
    Ghast,
    ZombiePigman,
    Enderman,
    CaveSpider,
    Silverfish,
    Blaze,
    MagmaCube,
    EnderDragon,
    Wither,
    Bat,
    Witch,
    Endermite,
    Guardian,
    Shulker,
    Pig,
    Sheep,
    Cow,
    Chicken,
    Squid,
    Wolf,
    Mooshroom,
    Snowman,
    Ocelot,
    IronGolem,
    Horse,
    Rabbit,
    PolarBear,
    Villager,
    Unknown
}

pub fn to_mob_type(id: u8) -> MobType {
    match id {
        50 => MobType::Creeper,
        51 => MobType::Skeleton,
        52 => MobType::Spider,
        53 => MobType::GiantZombie,
        54 => MobType::Zombie,
        55 => MobType::Slime,
        56 => MobType::Ghast,
        57 => MobType::ZombiePigman,
        58 => MobType::Enderman,
        59 => MobType::CaveSpider,
        60 => MobType::Silverfish,
        61 => MobType::Blaze,
        62 => MobType::MagmaCube,
        63 => MobType::EnderDragon,
        64 => MobType::Wither,
        65 => MobType::Bat,
        66 => MobType::Witch,
        67 => MobType::Endermite,
        68 => MobType::Guardian,
        69 => MobType::Shulker,
        90 => MobType::Pig,
        91 => MobType::Sheep,
        92 => MobType::Cow,
        93 => MobType::Chicken,
        94 => MobType::Squid,
        95 => MobType::Wolf,
        96 => MobType::Mooshroom,
        97 => MobType::Snowman,
        98 => MobType::Ocelot,
        99 => MobType::IronGolem,
        100 => MobType::Horse,
        101 => MobType::Rabbit,
        102 => MobType::PolarBear,
        120 => MobType::Villager,
        _ => MobType::Unknown
    }
}

#[derive(Debug, Eq, PartialEq)]
pub enum ObjectType {
    Boat,
    ItemStack,
    AreaEffectCloud,
    Minecrart,
    ActivatedTNT,
    EnderCrystal,
    Arrow,
    Snowball,
    Egg,
    FireBall,
    FireCharge,
    EnderPearl,
    WitherSkull,
    ShulkerBullet,
    FallenObject,
    ItemFrame,
    EyeOfEnder,
    Potion,
    ExpBottle,
    FireworkRocket,
    LeashKnot,
    ArmorStand,
    FishingFloat,
    SpectralArrow,
    DragonFireball,
    Unknown
}

pub fn to_object_type(id: u8) -> ObjectType {
    match id {
        1 => ObjectType::Boat,
        2 => ObjectType::ItemStack,
        3 => ObjectType::AreaEffectCloud,
        10 => ObjectType::Minecrart,
        50 => ObjectType::ActivatedTNT,
        51 => ObjectType::EnderCrystal,
        60 => ObjectType::Arrow,
        61 => ObjectType::Snowball,
        62 => ObjectType::Egg,
        63 => ObjectType::FireBall,
        64 => ObjectType::FireCharge,
        65 => ObjectType::EnderPearl,
        66 => ObjectType::WitherSkull,
        67 => ObjectType::ShulkerBullet,
        70 => ObjectType::FallenObject,
        71 => ObjectType::ItemFrame,
        72 => ObjectType::EyeOfEnder,
        73 => ObjectType::Potion,
        75 => ObjectType::ExpBottle,
        76 => ObjectType::FireworkRocket,
        77 => ObjectType::LeashKnot,
        78 => ObjectType::ArmorStand,
        90 => ObjectType::FishingFloat,
        91 => ObjectType::SpectralArrow,
        93 => ObjectType::DragonFireball,
        _ => ObjectType::Unknown,
    }
}
