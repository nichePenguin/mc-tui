use std::error::Error;

use crate::log;
use crate::buffered_reader::BufferedReader;
use crate::nbt::NbtData;

type Veci32 = Vec<i32>;
type VecSlot = Vec<Slot>;
type VecString = Vec<String>;
type Bytes = Box<[u8]>;

async fn read_nbt_data(data: &mut BufferedReader) -> Result<Option<NbtData>, Box<dyn Error + Send + Sync>> {
    let nbt_length = data.read_short().await?;
    if nbt_length == -1 {
        return Ok(None);
    }
    Ok(Some(NbtData::from_bytes(&data.read_bytes(nbt_length as usize).await?[..])))
}

#[derive(Debug)]
pub enum Slot {
    Empty,
    Item{id: i16, count: i8, damage: i16},
    ItemNbt{id: i16, count: i8, damage: i16, nbt: NbtData},
}

async fn read_slot(data: &mut BufferedReader) -> Result<Slot, Box<dyn Error + Send + Sync>> {
    let id = data.read_short().await?;
    if id == -1 {
        return Ok(Slot::Empty);
    }
    let count = data.read_byte().await?;
    let damage = data.read_short().await?;
    if let Some(nbt_data) = read_nbt_data(data).await? {
        return Ok(Slot::ItemNbt {
            id, count, damage,
            nbt: nbt_data
        })
    }
    Ok(Slot::Item {
        id,
        count,
        damage
    })
}

#[derive(Debug)]
pub struct Metadata {
    on_fire: bool,
    crouching: bool,
    riding: bool,
    sprinting: bool,
    acting: bool,
    invisible: bool,
    name: Option<String>,
    unknown: Vec<u8>
    // TODO other metadata
}

async fn read_metadata(data: &mut BufferedReader) -> Result<Metadata, Box<dyn Error + Send + Sync>> {
    let mut metadata = Metadata {
        on_fire: false,
        crouching: false,
        riding: false,
        sprinting: false,
        acting: false,
        invisible: false,
        name: None,
        unknown: vec![]
    };

    loop {
        let byte = data.read_ubyte().await?;
        if byte == 0x7F {
            return Ok(metadata)
        }
        let id = byte & 0x1F;
        let data_type = (byte & 0xE0) >> 5;
        if id == 0 {
            assert_eq!(data_type, 0);
            let flags = data.read_ubyte().await?;
            metadata.on_fire = flags & 0x01 != 0;
            metadata.crouching = flags & 0x02 != 0;
            metadata.riding = flags & 0x04 != 0;
            metadata.sprinting = flags & 0x08 != 0;
            metadata.acting = flags & 0x10 != 0;
            metadata.invisible = flags & 0x20 != 0;
            continue
        }
        if id == 5 {
            assert_eq!(data_type, 4);
            metadata.name = Some(data.read_string().await?);
            continue;
        }
        metadata.unknown.push(id);
        match data_type {
            0 => {data.read_byte().await?;},
            1 => {data.read_short().await?;},
            2 => {data.read_int().await?;},
            3 => {data.read_float().await?;},
            4 => {data.read_string().await?;},
            5 => {read_slot(data).await?;},
            6 => {
                let _x = data.read_int().await?;
                let _y = data.read_int().await?;
                let _z = data.read_int().await?;
            },
            _ => panic!("Unknown entity metadata field type: {data_type}")
        }
    }
}

#[derive(Debug)]
pub struct ObjectData {
    integer: i32,
    dx: Option<i16>,
    dy: Option<i16>,
    dz: Option<i16>,
}

async fn read_object_data(data: &mut BufferedReader) -> Result<ObjectData, Box<dyn Error + Send + Sync>> {
    let integer = data.read_int().await?;
    if integer != 0 {
        return Ok(ObjectData {
            integer,
            dx: Some(data.read_short().await?),
            dy: Some(data.read_short().await?),
            dz: Some(data.read_short().await?),
        });
    }
    Ok(ObjectData {
        integer,
        dx: None,
        dy: None,
        dz: None
    })
}

#[derive(Debug)]
pub struct MultiBlockChangeData {
    pub x: i32,
    pub z: i32,
    pub record_count: u16,
    pub bytes: Box<[u8]>
}

async fn read_multi_block_change_data(data: &mut BufferedReader) -> Result<MultiBlockChangeData, Box<dyn Error + Send + Sync>> {
    let x = data.read_int().await?;
    let z = data.read_int().await?;
    let record_count = data.read_ushort().await?;
    let len = data.read_int().await? as usize;
    let bytes = Box::from(data.read_bytes(len).await?);

    Ok(MultiBlockChangeData {
        x, z, record_count, bytes
    })
}

#[derive(Debug)]
pub struct ChunkMetainfo {
    pub x: i32,
    pub z: i32,
    pub primary: u16,
    pub add: u16
}

#[derive(Debug)]
pub struct ChunkDataBulk {
    pub column_count: u16,
    pub has_skylight: bool,
    pub compressed: Box<[u8]>,
    pub metainfo: Vec::<ChunkMetainfo>
}

#[derive(Debug)]
pub struct ChunkData {
    pub ground_up_continuous: bool,
    pub compressed: Box<[u8]>,
    pub metainfo: ChunkMetainfo,
}

async fn read_chunk_data(data: &mut BufferedReader) -> Result<ChunkData, Box<dyn Error + Send + Sync>> {
    let x = data.read_int().await?;
    let z = data.read_int().await?;
    let ground_up_continuous = data.read_bool().await?;
    let primary= data.read_ushort().await?;
    let add= data.read_ushort().await?;

    let len = data.read_int().await? as usize;
    let compressed = Box::from(data.read_bytes(len).await?);

    Ok(ChunkData {
        ground_up_continuous,
        compressed,
        metainfo: ChunkMetainfo {
            x,
            z,
            primary,
            add,
        }
    })
}

async fn read_chunk_data_bulk(data: &mut BufferedReader) -> Result<ChunkDataBulk, Box<dyn Error + Send + Sync>> {
    let column_count = data.read_ushort().await?;
    let len = data.read_int().await? as usize;
    let has_skylight = data.read_bool().await?;
    let compressed = Box::from(data.read_bytes(len).await?);
    let mut metainfo = Vec::<ChunkMetainfo>::new();
    for _ in 0..column_count {
        metainfo.push(
            ChunkMetainfo {
                x: data.read_int().await?,
                z: data.read_int().await?,
                primary: data.read_ushort().await?,
                add: data.read_ushort().await?
            }
        );
    }
    Ok(ChunkDataBulk {
        column_count,
        has_skylight,
        compressed,
        metainfo,
    })
}

#[derive(Debug)]
pub struct BlockOffsetRecords {
    pub offsets: Vec<(i8, i8, i8)>,
    pub dx: f32,
    pub dy: f32,
    pub dz: f32,
}

async fn read_block_offset_records(data: &mut BufferedReader) -> Result<BlockOffsetRecords, Box<dyn Error + Send + Sync>> {
    let count = data.read_int().await?;
    let mut offsets = Vec::<(i8, i8, i8)>::new();
    for _ in 0..count {
        offsets.push((
                data.read_byte().await?,
                data.read_byte().await?,
                data.read_byte().await?,
        ))
    }
    let dx = data.read_float().await?;
    let dy = data.read_float().await?;
    let dz = data.read_float().await?;
    Ok(BlockOffsetRecords {offsets, dx, dy, dz})
}

macro_rules! read_field {
    ($reader: ident, u8) => {
        $reader.read_ubyte().await?
    };
    ($reader: ident, i8) => {
        $reader.read_byte().await?
    };
    ($reader: ident, u16) => {
        $reader.read_ushort().await?
    };
    ($reader: ident, i16) => {
        $reader.read_short().await?
    };
    ($reader: ident, i32) => {
        $reader.read_int().await?
    };
    ($reader: ident, i64) => {
        $reader.read_long().await?
    };
    ($reader: ident, f32) => {
        $reader.read_float().await?
    };
    ($reader: ident, f64) => {
        $reader.read_double().await?
    };
    ($reader: ident, bool) => {
        $reader.read_bool().await?
    };
    ($reader: ident, String) => {
        $reader.read_string().await?
    };
    ($reader: ident, NbtData) => {
        read_nbt_data($reader).await?.expect("Packet expected to have NBT data")
    };
    ($reader: ident, Bytes) => {
        {
            let len = $reader.read_ushort().await? as usize;
            let bytes = $reader.read_bytes(len).await?;
            Box::from(bytes)
        }
    };
    ($reader: ident, Metadata) => {
        read_metadata($reader).await?
    };
    ($reader: ident, Slot) => {
        read_slot($reader).await?
    };
    ($reader: ident, ObjectData) => {
        read_object_data($reader).await?
    };
    ($reader: ident, ChunkData) => {
        read_chunk_data($reader).await?
    };
    ($reader: ident, ChunkDataBulk) => {
        read_chunk_data_bulk($reader).await?
    };
    ($reader: ident, MultiBlockChangeData) => {
        read_multi_block_change_data($reader).await?
    };
    ($reader: ident, BlockOffsetRecords) => {
        read_block_offset_records($reader).await?
    };
    ($reader: ident, VecSlot) => { 
        {
            let mut vec = Vec::<Slot>::new();
            let len = $reader.read_ushort().await?;
            for _ in 0..len {
                vec.push(read_slot($reader).await?);
            }
            vec
        }
    };
   ($reader: ident, Veci32) => { 
        {
            let len = $reader.read_ubyte().await?;
            let mut vec = Vec::<i32>::new();
            for _ in 0..len {
                vec.push($reader.read_int().await?)
            }
            vec
        }
    };
    ($reader: ident, VecString) => { 
        {
            let len = $reader.read_ubyte().await?;
            let mut vec = Vec::<String>::new();
            for _ in 0..len {
                vec.push($reader.read_string().await?)
            }
            vec
        }
    };
}

macro_rules! write_field {
    ($vec: ident, $field: ident, u8) => {
        $vec.push($field);
    };
    ($vec: ident, $field: ident, String) => {
        let length = $field.len() as i16;
        write_field!($vec, length, i16);
        $field.encode_utf16().flat_map(|i| i.to_be_bytes()).for_each(|x| $vec.push(x));
    };
    ($vec: ident, $field: ident, Bytes) => {
        let length = $field.len() as u16;
        write_field!($vec, length, u16);
        $field.iter().for_each(|x| $vec.push(*x));
    };
    ($vec: ident, $field: ident, bool) => {
        if $field {
            $vec.push(1u8);
        } else {
            $vec.push(0u8)
        }
    };
    ($vec: ident, $field: ident, NbtData) => {
        let len = $field.len() as i32;
        write_field!($vec, len, i32);
        let bytes = $field.to_bytes();
        write_field!($vec, bytes, Bytes);
    };
    ($vec: ident, $field: ident, Slot) => {
        match $field {
            Slot::Empty => {
                let id = -1i16;
                write_field!($vec, id, i16);
            },
            Slot::Item {id, count, damage} => {
                write_field!($vec, id, i16);
                write_field!($vec, count, i8);
                write_field!($vec, damage, i16);
            }
            Slot::ItemNbt {id, count, damage, nbt} => {
                write_field!($vec, id, i16);
                write_field!($vec, count, i8);
                write_field!($vec, damage, i16);
                write_field!($vec, nbt, NbtData);
            }
        };
    };
    ($vec: ident, $field: ident, ChunkData) => {
        panic!("chunk data serialization is not supported");
    };
    ($vec: ident, $field: ident, ChunkDataBulk) => {
        panic!("chunk data serialization is not supported");
    };
    ($vec: ident, $field: ident, MultiBlockChangeData) => {
        panic!("multiblock change data serialization is not supported");
    };
    ($vec: ident, $field: ident, BlockOffsetRecords) => {
        panic!("block offset serialization is not supported");
    };
    ($vec: ident, $field: ident, ObjectData) => {
        panic!("object data serialization is not supported");
    };
    ($vec: ident, $field: ident, Metadata) => {
        panic!("metadata serialization is not supported");
    };
    ($vec: ident, $field: ident, VecSlot) => {
        panic!("vector serialization is not supported");
    };
    ($vec: ident, $field: ident, Veci32) => {
        panic!("vector serialization is not supported");
    };
    ($vec: ident, $field: ident, VecString) => {
        panic!("vector serialization is not supported");
    };
    ($vec: ident, $field: ident, $type: ty) => {
        $field.to_be_bytes().into_iter().for_each(|x| $vec.push(x));
    };
}

macro_rules! protocol {
    ($($packet_type: ident <$packet_id: literal> { $($field: ident: $field_type: tt),+ }),+) => {
        // TODO optimize packet size?
        #[derive(Debug)]
        pub enum Packet {
            $(
                $packet_type {
                    $(
                        $field: $field_type,
                    )*
                },
            )*
        }

        pub async fn read(reader: &mut BufferedReader) -> Packet {
            match try_read(reader).await {
                Ok(packet) => {
                    return packet;
                },
                Err(e) => {
                    log::error!("Error while reading packet: {}", e.to_string());
                    panic!("Error while reading packet: {}", e.to_string());
                }
            }
        }
        pub async fn try_read(reader: &mut BufferedReader) -> Result<Packet, Box<dyn Error + Send + Sync>> {
            let id = reader.read_ubyte().await?;
            match id {
                $(
                    $packet_id => {
                        Ok(
                            Packet::$packet_type {
                                $(
                                    $field: read_field!(reader, $field_type),
                                )*
                            }
                        )
                    }
                )*
                _ => panic!("Unknown packet id: {}", id)
            }
        }
        pub fn write(packet: Packet) -> Vec<u8> {
            match packet {
                $(
                    Packet::$packet_type{$($field,)*} => {
                        let mut out: Vec<u8> = vec![$packet_id];
                        $(
                            write_field!(out, $field, $field_type);
                        )*
                        out
                    },
                )*
            }
        }
    }
}

protocol! (
    KeepAlive<0x00> {
        keep_alive_id: i32
    },
    LoginRequest<0x01> {
        entity_id: i32,
        level_type: String,
        game_mode: i8,
        dimension: i8,
        difficulty: i8,
        unused: i8,
        max_players: i8
    },
    Handshake<0x02> {
        protocol_version: u8,
        username: String,
        host: String,
        port: i32
    },
    ChatMessage<0x03> {
        message: String
    },
    TimeUpdate<0x04> {
        age: i64,
        time: i64
    },
    EntityEquipment<0x05> {
        eid: i32,
        slot: i16,
        item: Slot
    },
    SpawnPosition<0x06> {
        x: i32,
        y: i32,
        z: i32
    },
    UseEntity<0x07> {
        user: i32,
        target: i32,
        mouse_button: bool
    },
    UpdateHealth<0x08> {
        health: i16,
        food: i16,
        saturation: f32
    },
    Respawn<0x09> {
        dim: i32,
        difficulty: u8,
        game_mode: u8,
        height: i16,
        level_type: String
    },
    Player<0x0A> {
        on_ground: bool
    },
    PlayerPosition<0x0B> {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        on_ground: bool
    },
    PlayerLook<0x0C> {
        yaw: f32,
        pitch: f32,
        on_ground: bool
    },
    PlayerPositionAndLook<0x0D> {
        x: f64,
        y: f64,
        stance: f64,
        z: f64,
        yaw: f32,
        pitch: f32,
        on_ground: bool
    },
    PlayerDigging<0x0E> {
        status: u8,
        x: i32,
        y: u8,
        z: i32,
        face: u8
    },
    PlayerBlockPlacement<0x0F>{
        x: i32,
        y: u8,
        z: i32,
        dir: u8,
        item: Slot,
        cur_x: u8,
        cur_y: u8,
        cur_z: u8
    },
    HeldItemChange<0x10> {
        slot_id: i16
    },
    UseBed<0x11> {
        eid: i32,
        unknown: u8,
        x: i32,
        y: u8,
        z: i32
    },
    Animation<0x12> {
        eid: i32,
        anim: u8
    },
    EntityAction<0x13> {
        eid: i32,
        action: u8
    },
    SpawnNamedEntity<0x14> {
        eid: i32,
        name: String,
        x: i32,
        y: i32,
        z: i32,
        yaw: u8,
        pitch: u8,
        item: i16,
        metadata: Metadata
    },
    CollectItem<0x16> {
        collected: i32,
        collector: i32
    },
    SpawnObject<0x17> {
        eid: i32,
        obj_type: u8,
        x: i32,
        y: i32,
        z: i32,
        pitch: u8,
        yaw: u8,
        object_data: ObjectData
    },
    SpawnMob<0x18> {
        eid: i32,
        mob_type: u8,
        x: i32,
        y: i32,
        z: i32,
        pitch: i8,
        head_pitch: i8,
        yaw: i8,
        dx: i16,
        dy: i16,
        dz: i16,
        metadata: Metadata
    },
    SpawnPainting<0x19> {
        eid: i32,
        title: String,
        x: i32,
        y: i32,
        z: i32,
        dir: i32
    },
    SpawnExperienceOrb<0x1A> {
        eid: i32,
        x: i32,
        y: i32,
        z: i32,
        count: u16
    },
    EntityVelocity<0x1C> {
        eid: i32,
        dx: i16,
        dy: i16,
        dz: i16
    },
    EntityDestroy<0x1D> {
        ids: Veci32
    },
    Entity<0x1E> {
        eid: i32
    },
    EntityRelativeMove<0x1F> {
        eid: i32,
        dx: i8,
        dy: i8,
        dz: i8
    },
    EntityLook<0x20> {
        eid: i32,
        yaw: i8,
        pitch: i8
    },
    EntityLookAndRelativeMove<0x21> {
        eid: i32,
        dx: i8,
        dy: i8,
        dz: i8,
        yaw: i8,
        pitch: i8
    },
    EntityTeleport<0x22> {
        eid: i32,
        x: i32,
        y: i32,
        z: i32,
        yaw: i8,
        pitch: i8
    },
    EntityHeadLook<0x23> {
        eid: i32,
        yaw: i8
    },
    EntityStatus<0x26> {
        eid: i32,
        status: u8
    },
    EntityAttach<0x27> {
        eid: i32,
        vehicle_eid: i32
    },
    EntityMetadata<0x28> {
        eid: i32,
        metadata: Metadata
    },
    EntityEffect<0x29> {
        eid: i32,
        effect_id: u8,
        amplifier: u8,
        duration: i16
    },
    RemoveEntityEffect<0x2A> {
        eid: i32,
        effect_id: u8
    },
    SetExperience<0x2B> {
        bar: f32,
        level: i16,
        total: i16
    },
    ChunkData<0x33> {
        chunk_data: ChunkData
    },
    MultiBlockChange<0x34> {
        change_data: MultiBlockChangeData
    },
    BlockChange<0x35> {
        x: i32,
        y: u8,
        z: i32,
        block_type: u16,
        block_meta: u8
    },
    BlockAction<0x36> {
        x: i32,
        y: i16,
        z: i32,
        hb: u8,
        lb: u8,
        block_id: i16
    },
    BlockBreakAnimation<0x37> {
        eid: i32,
        x: i32,
        y: i32,
        z: i32,
        destroy_stage: u8
    },
    ChunkDataBulk<0x38> {
        column_data: ChunkDataBulk 
    },
    Explosion<0x3C> {
        x: f64,
        y: f64,
        z: f64,
        radius: f32,
        block_offsets: BlockOffsetRecords
    },
    SoundOrParticleEffect<0x3D> {
        effect_id: i32,
        x: i32,
        y: u8,
        z: i32,
        data: i32,
        absolute: bool
    },
    NamedSoundEffect<0x3E> {
        name: String,
        x: i32,
        y: i32,
        z: i32,
        volume: f32,
        pitch: u8
    },
    Particle<0x3F> {
        name: String,
        x: f32,
        y: f32,
        z: f32,
        dx: f32,
        dy: f32,
        dz: f32,
        speed: f32,
        count: i32
    },
    ChangeGameState<0x46> {
        reason: u8,
        game_mode: u8
    },
    SpawnGlobalEntity<0x47> {
        eid: i32,
        etype: u8,
        x: i32,
        y: i32,
        z: i32
    },
    OpenWindow<0x64> {
        window_id: u8,
        inv_type: u8,
        title: String,
        slots: u8,
        use_title: bool
    },
    CloseWindow<0x65> {
        window_id: u8
    },
    ClickWindow<0x66> {
        window_id: u8,
        slot: u16,
        button: u8,
        action: u16,
        mode: u8,
        item: Slot
    },
    SetSlot<0x67> {
        window_id: i8,
        slot: i16,
        item: Slot
    },
    SetWindowItems<0x68> {
        window_id: u8,
        slots: VecSlot
    },
    UpdateWindowProperty<0x69> {
        window_id: u8,
        property: i16,
        value: i16
    },
    ConfirmTransaction<0x6A> {
        window_id: u8,
        action_number: i16,
        is_accepted: bool
    },
    CreativeInventoryAction<0x6B> {
        slot: u16,
        item: Slot
    },
    EnchantItem<0x6C> {
        window_id: u8,
        enchantement: u8
    },
    UpdateSign<0x82> {
        x: i32,
        y: u16,
        z: i32,
        text_1: String,
        text_2: String,
        text_3: String,
        text_4: String
    },
    ItemData<0x83> {
        item_type: i16,
        item_id: i16,
        text: Bytes 
    },
    UpdateTileEntity<0x84> {
        x: i32,
        y: u16,
        z: i32,
        action: u8,
        nbt: NbtData
    },
    IncrementStat<0xC8> {
        stat_id: i32,
        amount: i8
    },
    PlayerListItem<0xC9> {
        name: String,
        online: bool,
        pink: u16
    },
    PlayerAbilities<0xCA> {
        flags: u8,
        flying_speed: u8,
        walking_speed: u8
    },
    TabComplete<0xCB> {
        text: String
    },
    ClientSettings<0xCC> {
        locale: String,
        view_distance: u8,
        chat_flags: u8,
        difficulty: u8,
        show_cape: bool
    },
    ClientStatuses<0xCD> {
        payload: u8
    },
    ScoreboardObjective<0xCE> {
        name: String,
        value: String,
        cr: u8
    },
    UpdateScore<0xCF> {
        item_name: String,
        ur: u8,
        score_name: String,
        value: i32
    },
    DisplayScoreboard<0xD0> {
        pos: u8,
        name: String
    },
    Teams<0xD1> {
        name: String,
        mode: u8,
        display_name: String,
        prefix: String,
        suffix: String,
        firendly_fire: u8,
        player_count: u16,
        players: VecString
    },
    PluginMessage<0xFA> {
        channel: String,
        data: Bytes 
    },
    EncryptionKeyResponse<0xFC> {
        shared_secret: Bytes, 
        verify_token: Bytes
    },
    EncryptionKeyRequest<0xFD> {
        server_id: String,
        pbkey: Bytes,
        verify_token: Bytes
    },
    ServerListPing<0xFE> {
        magic: u8
    },
    Disconnect<0xFF> {
        reason: String
    }
);
