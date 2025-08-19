use std::collections::HashMap;
use miniz_oxide::inflate::decompress_to_vec_zlib;
use ratatui::buffer::Cell;
use ratatui::style::Color;

use crate::util::pos_add;
use crate::game::{GlobalContext, GameState};
use crate::packets::{
    ChunkData,
    ChunkDataBulk,
    ChunkMetainfo,
    MultiBlockChangeData
};

const BYTE_CHUNK: usize = 16*16*16;
const HALFBYTE_CHUNK: usize = 16*16*16/2;

const AIR_ALPHA: f64 = 0.24;
const AIR_COLOR: (u8, u8, u8) = (0, 0, 0);

const MAX_RENDER_DEPTH: i32 = 3;
const LIGHT_ENABLED: bool = false;
const DEPTH_ENABLED: bool = true;

#[derive(Debug, Clone)]
pub struct Chunk {
    y: u8,
    blocks: [Block; BYTE_CHUNK],
}

impl Chunk {
    pub fn empty(y: u8) -> Self {
        Self {
            y,
            blocks: [Block::AIR; BYTE_CHUNK]
        }
    }
}

#[derive(Clone)]
pub struct ChunkColumn {
    x: i32,
    z: i32,
    chunks: Vec::<Option<Chunk>>,
    biome: [u8; 256]
}

impl ChunkColumn {
    pub fn get_block(&self, pos: (i32, i32, i32)) -> Block {
        let y = pos.1 as usize;
        if y > self.chunks.len()*16 {
            return Block::AIR;
        }
        let x = (pos.0 & 0xF) as usize;
        let z = (pos.2 & 0xF) as usize;
        if let Some(Some(chunk)) = self.chunks.get(y / 16) {
            let y = y % 16;
            return chunk.blocks[x + z*16 + y*16*16];
        }
        Block::AIR
    }

    pub fn set_block(&mut self, pos: (i32, i32, i32), block: Block) {
        let y = pos.1 as usize;
        if y > self.chunks.len()*16 {
            eprintln!("invalid set_block");
            return
        }
        let x = (pos.0 & 0xF) as usize;
        let z = (pos.2 & 0xF) as usize;
        let chunk_y = (y / 16) as usize;
        let y = (pos.1 & 0xF) as usize;

        if let Some(Some(chunk)) = self.chunks.get_mut(chunk_y) {
            let y = y % 16;
            chunk.blocks[x + z*16 + y*16*16] = block
        } else {
            let mut chunk = Chunk::empty(chunk_y as u8);
            chunk.blocks[x + z*16 + y*16*16] = block;
            self.chunks[chunk_y] = Some(chunk);
        }
    }

    pub fn new(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            chunks: (0..16).map(|_| None).collect(),
            biome: [0u8; 256]
        }
    }

    pub fn empty(x: i32, z: i32) -> Self {
        Self {
            x,
            z,
            chunks: vec![],
            biome: [0u8; 256]
        }
    }
}

#[derive(Clone)]
pub struct World {
    columns: HashMap::<(i32, i32), ChunkColumn>,
}

impl World {
    pub fn new() -> Self {
        World {
            columns: HashMap::new(),
        }
    }

    pub async fn get_slice_render(
        &self,
        width: u16,
        height: u16,
        ctx: &GlobalContext) -> (Box<[Cell]>, (u16, u16)) 
    {
        let global_camera = ctx.camera;
        let mut render = vec![];
        for y in 0..height {
            for x in 0..width {
                let pos = (global_camera.0 - (width/2) as i32 + x as i32, global_camera.1, global_camera.2 - (height/2) as i32 + y as i32);
                render.push(self.get_block_render(pos, ctx).await);
            }
        }
        (render.into_boxed_slice(), (width/2, height/2))
    }

    pub async fn get_block_render(&self, pos: (i32, i32, i32), ctx: &GlobalContext) -> Cell {
        if let GameState::WorldLook = ctx.mode { // TODO move to separate render layer
            if ctx.camera == pos && ctx.tick % 10 > 4 {
                return BlockRender::CURSOR.into();
            }
        }
        for player in ctx.players.iter() { // TODO remove when players are added as entities
            let world_pos = player.read().await.world_pos();
            let world_pos_top = pos_add(world_pos, (0, 1, 0));
            if pos == world_pos || pos == world_pos_top {
                return BlockRender::PLAYER.into();
            }
        }
        let mut block = self.get_block(pos);
        if !DEPTH_ENABLED {
            return to_render_block(&block, ctx).into();
        }

        let mut fg_depth = 0;

        while block.is_air() {
            fg_depth += 1;
            if fg_depth > MAX_RENDER_DEPTH {
                return BlockRender::VOID.into();
            }
            block = self.get_block((pos.0, pos.1 - fg_depth, pos.2));
        }

        let render_fg = to_render_block(&block, ctx);
        let mut bg_depth = fg_depth;
        let mut render_bg = render_fg;
        while render_bg.bg.is_none() {
            bg_depth += 1;
            if bg_depth > MAX_RENDER_DEPTH {
                render_bg = BlockRender::VOID;
                break;
            }
            render_bg = to_render_block(&self.get_block((pos.0, pos.1 - bg_depth, pos.2)), ctx);
        }

        BlockRender {
            character: render_fg.character,
            fg: apply_air(render_fg.fg, fg_depth),
            bg: Some(apply_air(render_bg.bg.unwrap(), bg_depth))
        }.into()
    }

    pub fn get_block(&self, pos: (i32, i32, i32)) -> Block {
        if pos.1 < 0 {
            return Block::AIR; // Void ??
        }
        let chunk_pos = (
                pos.0 >> 4,
                pos.2 >> 4
            );
        if !self.columns.contains_key(&chunk_pos) {
            return Block::AIR;
        }
        let chunk = self.columns.get(&chunk_pos).unwrap();
        chunk.get_block(pos)
    }

    pub fn set_chunk(&mut self, data: ChunkData) {
        self.parse(
            &decompress_to_vec_zlib(&data.compressed).unwrap()[..],
            &[data.metainfo],
            true,
            data.ground_up_continuous);
    }

    pub fn set_chunk_bulk(&mut self, data: &ChunkDataBulk) {
        self.parse(
            &decompress_to_vec_zlib(&data.compressed).unwrap()[..],
            &data.metainfo[..],
            data.has_skylight,
            true);
    }

    pub fn set_block_multiple(&mut self, data: &MultiBlockChangeData) {
        let chunk_x = data.x;
        let chunk_z = data.z;
        let column = self.columns.get_mut(&(chunk_x, chunk_z)).unwrap();
        for i in 0..data.record_count {
            let i = (i*4) as usize;
            let a = data.bytes[i];
            let b = data.bytes[i+1];
            let c = data.bytes[i+2];
            let d = data.bytes[i+3];

            let x = ((a & 0xF0) >> 4) as i32;
            let z = (a & 0x0F) as i32;
            let y = b as i32;
            let id = ((c as u16) << 4) + ((d as u16 & 0xF0) >> 4);
            let meta = d & 0x0F;
            let mut block = Block::new();
            block.id = id;
            block.metadata = meta;
            column.set_block((x as i32 + chunk_x*16, y as i32, z as i32 + chunk_z*16), block)
        }
    }

    pub fn set_block(&mut self, x: i32, z: i32, y: u8, block_type: u16, block_meta: u8) {
        let chunk_x = x.div_floor(16);
        let chunk_z = z.div_floor(16);
        if !self.columns.contains_key(&(chunk_x, chunk_z)) {
            self.columns.insert((chunk_x, chunk_z), ChunkColumn::new(chunk_x, chunk_z));
        }
        let column = self.columns.get_mut(&(chunk_x, chunk_z)).unwrap();
        let mut block = Block::new();
        block.id = block_type;
        block.metadata = block_meta;
        column.set_block((x, y as i32, z), block);
    }

    pub fn parse(
        &mut self,
        chunk_data: &[u8],
        metadata: &[ChunkMetainfo],
        skylight: bool,
        ground_up: bool
    ) { 
        let data_total = chunk_data.len();
        let mut data_consumed = 0;
        let data_iter = &mut chunk_data.into_iter();
        for ChunkMetainfo {x, z, primary, add } in metadata {
            let mut column = ChunkColumn::empty(*x, *z);
            for y in 0..16 {
                if primary & (1 << y) != 0 {
                    let chunk = Chunk {
                        y,
                        blocks: [Block::new(); BYTE_CHUNK],
                    };
                    column.chunks.push(Some(chunk));
                } else {
                    column.chunks.push(None);
                }
            }
            for chunk in column.chunks.iter_mut().filter(|c| c.is_some()).map(|c| c.as_mut().unwrap()) {
                chunk.blocks.iter_mut().zip(data_iter.take(BYTE_CHUNK))
                    .for_each(|(block, id)| block.id = *id as u16);
                data_consumed += BYTE_CHUNK;
        }

        for chunk in column.chunks.iter_mut().filter(|c| c.is_some()).map(|c| c.as_mut().unwrap()) {
            chunk.blocks.chunks_mut(2).zip(data_iter.take(HALFBYTE_CHUNK))
                .for_each(|(block, metadata)| {
                    block[0].metadata = metadata & 0x0F;
                    block[1].metadata = (metadata & 0xF0) >> 4;
                });
            data_consumed += HALFBYTE_CHUNK;
            }

            for chunk in column.chunks.iter_mut().filter(|c| c.is_some()).map(|c| c.as_mut().unwrap()) {
                chunk.blocks.chunks_mut(2).zip(data_iter.take(HALFBYTE_CHUNK))
                    .for_each(|(block, light)| {
                        block[0].light = light & 0x0F;
                        block[1].light = (light & 0xF0) >> 4;
                    });
                data_consumed += HALFBYTE_CHUNK;
            }

            for chunk in column.chunks.iter_mut().filter(|c| c.is_some()).map(|c| c.as_mut().unwrap()) {
                if skylight {
                    data_consumed += HALFBYTE_CHUNK;
                    chunk.blocks.chunks_mut(2).zip(data_iter.take(HALFBYTE_CHUNK))
                        .for_each(|(block, skylight)| {
                            block[0].skylit = skylight & 0x0F;
                            block[1].skylit = (skylight & 0xF0) >> 4;
                        });
                }
            }

            for chunk in column.chunks.iter_mut().filter(|c| c.is_some()).map(|c| c.as_mut().unwrap()) {
                if add & (1 << chunk.y) != 0 {
                    data_consumed += HALFBYTE_CHUNK;
                    chunk.blocks.chunks_mut(2).zip(data_iter.take(HALFBYTE_CHUNK))
                        .for_each(|(block, add_id)| {
                            block[0].id += (add_id & 0x0F) as u16;
                            block[1].id += ((add_id& 0xF0) >> 4) as u16;
                        });
                }
            }

            if ground_up {
                data_consumed += 256;
                column.biome.iter_mut().zip(data_iter.take(256))
                    .for_each(|(biome, value)| *biome = *value)
            }
            self.columns.insert((*x, *z), column);
        }
        assert_eq!(data_total, data_consumed);
        assert_eq!(data_iter.count(), 0);
    }
}

fn apply_air(color: (u8, u8, u8), depth: i32) -> (u8, u8, u8){
    let alpha = AIR_ALPHA * depth as f64;
    (
        (alpha * AIR_COLOR.0 as f64 + (1.0 - alpha) * color.0 as f64) as u8,
        (alpha * AIR_COLOR.1 as f64 + (1.0 - alpha) * color.1 as f64) as u8,
        (alpha * AIR_COLOR.2 as f64 + (1.0 - alpha) * color.2 as f64) as u8,
    )
}

// TODO separate block from its rendering?
#[derive(Clone, Copy, Debug)]
pub struct BlockRender {
    pub fg: (u8, u8, u8),
    pub bg: Option<(u8, u8, u8)>,
    pub character: char
}

impl Into<Cell> for BlockRender {
    fn into(self) -> Cell {
        let mut cell = Cell::EMPTY;
        cell.set_char(self.character)
            .set_fg(Color::Rgb(
                self.fg.0,
                self.fg.1,
                self.fg.2)
        );
        if let Some(bg) = self.bg {
            cell.set_bg(Color::Rgb(
                bg.0,
                bg.1,
                bg.2
            ));
        }
        cell
    }
}

impl BlockRender {
    pub const CURSOR: BlockRender = BlockRender { // TODO move to another render layer
        fg: (255, 90, 90),
        bg: None,
        character: 'X'
    };

    pub const PLAYER: BlockRender = BlockRender {
        fg: (255, 0, 0),
        bg: None,
        character: '@'
    };

    pub const VOID: BlockRender = BlockRender {
        fg: (0, 0, 0),
        bg: Some((0, 0, 0)),
        character: ' '
    };

    pub const UNKNOWN: BlockRender = BlockRender {
        fg: (255, 0, 255),
        bg: Some((128, 0, 128)),
        character: '?'
    };
}

#[derive(Debug)]
pub struct BlockInfo {
    pub id: u16,
    pub is_solid: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct Block {
    pub id: u16,
    metadata: u8,
    light: u8,
    skylit: u8,
}

impl Block {
    pub fn new() -> Self {
        Block {
            id: 0,
            metadata: 0,
            light: 0,
            skylit: 0 
        }
    }
    pub fn is_air(&self) -> bool {
        self.id == 0
    }

    const AIR: Block = Block {
        id: 0,
        metadata: 0,
        light: 0,
        skylit: 0
    };
}

fn to_render_block(block: &Block, ctx: &GlobalContext) -> BlockRender {
    let render_dict = HashMap::<(u16, u8), BlockRender>::new();
    // TODO load from resources / blockinfo 
    let key = &(block.id, block.metadata);
    if !render_dict.contains_key(key) {
        return to_render_block_old(block.id, block.metadata, ctx);
    }

    if LIGHT_ENABLED {
        // TODO light
    }
    return render_dict.get(&key).unwrap_or(&BlockRender::VOID).clone()
}

fn color(r: u8, g: u8, b: u8) -> Option<(u8, u8, u8)> {
    Some((r, g, b))
}

fn to_render_block_old(id: u16, meta: u8, ctx: &GlobalContext) -> BlockRender {
    // TODO move to resources / blockinfo
    let (character, fg, bg) = match id {
        0 => ('█', Some(AIR_COLOR), None),
        1 => ('█', color(158, 158, 158), color(158, 158, 158)),
        2 => ('█', color(10, 215, 10), color(10, 215, 10)),
        3 => ('█', color(156, 112, 76), color(156, 112, 76)),
        4 => ('▒', color(128, 128, 128), color(108, 108, 108)),
        5 => ('█', color(188, 152, 98), color(204, 205, 139)),
        6 => ('ፑ', color(156, 112, 76), color(10, 215, 10)),
        7 => ('▒', color(128, 128, 128),color(24, 24, 24)),
        8 => ('~', color(87, 151, 255), color(61, 64, 255)),
        9 => ('≈', color(87, 151, 255), color(61, 64, 255)),
        10 => ('~', color(255, 213, 0), color(255, 48, 0)),
        11 => ('≈', color(255, 213, 0), color(255, 48, 0)),
        12 => ('█', color(254, 255, 189), color(254, 255, 189)),
        13 => ('#', color(117, 112, 110), color(196, 185, 183)),
        14 => ('&', color(212, 158, 158), color(158, 158, 158)),
        15 => ('&', color(212, 158, 158), color(158, 158, 158)),
        16 => ('&', color(25, 25, 25), color(158, 158, 158)),
        17 => ('O', color(230, 172, 110), color(110, 69, 45)),
        18 => ('░', color(12, 223, 12), None),
        20 => ('‘', color(0, 255, 255), None), // glass
        21 => ('&', color(0, 69, 181), color(158, 158, 158)),
        24 => ('█', color(204, 205, 139), color(204, 205, 139)),
        25 => ('░', color(200, 0, 65), color(100, 84, 84) ), // note block
        26 => ('▄', color(224, 28, 28), color(224, 224, 224)),
        27 => {
            let character = match meta & 0b111{
                0 => '║',
                1 => '═',
                2 => '═',//'╘',
                3 => '═',//'╛',
                4 => '║',//'╖',
                5 => '║',//'╜',
                6 => '╔',
                7 => '╗',
                8 => '╝',
                9 => '╚',
                _ => panic!("unknown rail metadata")
            };
            let power = if meta & 0b1000 == 8 {
                color(235, 205, 0)
            } else {
                color(95, 65, 0)
            };
            (character, power, None)
        }, // powered rail
        29 => {
            let power = if meta & 0b1000 == 8 {
                color(255, 58, 58)
            } else {
                color(128, 128, 128)
            };
            let character = match meta & 0b0111 {
                0 => '○',
                1 => '●',
                2 => '↥',
                3 => '↧',
                4 => '↤',
                5 => '↦',
                _ => '?'
            };
            (character, power, color(108, 208, 108))
        }, // sticky piston 
        30 => ('Ж', color(255, 255, 255), None),
        31 => ('⍦', color(156, 112, 76), None),
        33 => {
            let power = if meta & 0b1000 == 8 {
                color(255, 58, 58)
            } else {
                color(128, 128, 128)
            };
            let character = match meta & 0b0111 {
                0 => '○',
                1 => '●',
                2 => '↥',
                3 => '↧',
                4 => '↤',
                5 => '↦',
                _ => '?'
            };
            (character, power, color(108, 108, 108))

        } // piston
        34 => {
            let character = match meta & 0b0111 {
                0 => '•',
                1 => '█',
                2 => '⊤',
                3 => '⊥',
                4 => '⊢',
                5 => '⊣',
                _ => '?'
            };
            (character, color(188, 152, 98), None)
        } // sticky piston head
        35 => ('░', color(235, 235, 235), color(205, 205, 205)),
        37 => ('❀', color(255, 255, 0), color(10, 215, 10)),
        38 => ('⚘', color(255, 0, 0), color(10, 215, 10)),
        39 => ('Ⱄ', color(156, 112, 76), color(10, 215, 10)),
        42 => ('■', color(214, 215, 216), color(146, 146, 145)), // iron block
        43 => match meta {
            0 => ('─', color(158, 158, 158), color(198, 198, 198)),
//            1 =>,
//            2 =>,
//            3 =>,
            4 => ('▤', color(250, 234, 225), color(193, 74, 9)),
//            5 =>,
//            6 =>,
//            7 =>,
//            8 =>,
//            9 =>,
//            10 =>,
//            11 =>,
//            12 =>,
//            13 =>,
//            14 =>,
//            15 =>,
            _ => ('?', color(255,  255, 0), color(200, 200, 0))
        }, // double slab
        44 => ('▄', color(158, 158, 158), None),
        45 => ('▤', color(250, 234, 225), color(193, 74, 9)), // bricks
        47 => ('▤', color(188, 152, 98), None), //bookshelf
        48 => ('▒', color(128, 255, 128), color(108, 108, 108)),
        49 => ('▒', color(13, 0, 23),color(25, 0, 37)),
        50 => ('༈', color(230, 210, 0), None),
        51 => match ctx.tick % 5 / 2 {
            0 => ('‼', color(255, 128, 0), None),
            1 => ('‼', color(255, 0, 0), None),
            2 => (' ', color(255, 0, 0), None),
            _ => panic!("huh")
        },
        52 => ('#', color(200, 30, 200), color(180,10, 180)),
        53 => ('▙', color(188, 152, 98), None), // wooden stair
        54 => ('⌺', color(204, 205, 139), color(110,69,45)),
        55 => {
            let power = meta * (200/15) + 50;
            ('┼', color(power, 0, 0), None)
        },
        56 => ('◆', color(125, 251, 255), color(158, 158, 158)),
        58 => ('#', color(110, 69, 45), color(230, 172, 110)),
        61 => ('⌸', color(158, 158, 158), color(108, 108, 108)),
        63 => ('▬', color(188, 152, 98), None), // sign
        64 => ('+', color(204, 205, 139), None),
        65 => ('▤', color(188, 152, 98), None), // ladder
        66 => {
            let character = match meta {
                0 => '║',
                1 => '═',
                2 => '═',//'╘',
                3 => '═',//'╛',
                4 => '║',//'╖',
                5 => '║',//'╜',
                6 => '╔',
                7 => '╗',
                8 => '╝',
                9 => '╚',
                _ => panic!("unknown rail metadata")
            };
            (character, color(214, 215, 216), None)
        }, // rail
        68 => ('▬', color(188, 152, 98), None), //wall sign
        67 => ('▙', color(108, 108, 108), None),
        70 => ('⎽', color(158, 158, 158), None), // pressure plate
        72 => ('⎽', color(188, 152, 98), None), // pressure plate (wood)
        73 => ('&', color(255, 32, 32), color(158, 158, 158)),
        75 => ('༈', color(80, 10, 10), None),
        76 => ('༈', color(230, 10, 10), None),
        77 => ('▪', color(158, 158, 158), None ), // stone button
        78 => ('▒', color(235, 235, 255),color(215, 215, 235)),
        79 => ('▒', color(91, 115, 255), color(215, 235, 255)),
        82 => ('▒', color(157, 162, 174), color(132, 138, 150)),
        83 => ('⊪', color(50, 225, 50), None),
        85 => ('┼', color(188, 152, 98), None), // fence
        86 => ('ϖ', color(252, 161, 3), color(201, 110, 0)),
        87 => ('▒', color(97, 7, 7), color(93, 53, 53)), //netherrack
        89 => ('▒', color(235, 205, 0), color(200, 185, 0)), // glowstone
        90 => ('▋', color(225, 10, 225), None),
        92 => ('░', color(255, 0, 0), color(255, 255, 255)), // cake
        93 => {
            let dir = meta & 0b011;
            let delay = meta & 0xF0;
            let character = match dir {
                0b00 => '⍐',
                0b01 => '⍈',
                0b10 => '⍗',
                0b11 => '⍇',
                _ => '?'
            };
            (character, color(128, 128, 128), color(158, 158, 158))
        },
        94 => {
            let dir = meta & 0b011;
            let delay = meta & 0xF0;
            let character = match dir {
                0b00 => '⍐',
                0b01 => '⍈',
                0b10 => '⍗',
                0b11 => '⍇',
                _ => '?'
            };
            (character, color(255, 58, 58), color(158, 158, 158))
        }
        98 => ('▞', color(158, 158, 158), color(138, 138, 138)), //stone bricks
        101 => ('┼', color(146, 146, 145), None),
        102 => ('┼', color(225, 225, 255), None),
        106 => ('⸾', color(12, 223, 12), None),// vine
        108 => ('▙', color(193, 74, 9), None), // brick stairs
        109 => ('▙', color(138, 138, 138), None), // stone brick stairs
        112 => ('▞', color(81, 21, 21), color(114, 50, 50)), // nether brick
        113 => ('┼', color(81, 21, 21), None),// nether brick fence
        114 => ('▙', color(81, 21, 21), None), // nether brick stairs
        123 => ('☼', color(235, 205, 0), color(55, 25, 25)), // redstone lamp (unlit)
        124 => ('☼', color(95, 65, 0), color(55, 25, 25)), // redstone lamp (lit)
        125 => ('▄', color(230, 172, 110), None),
        126 => ('█', color(230, 172, 110), color(230, 172, 110)),
        133 => ('☼', color(100, 237, 146), color(60, 142, 87)), // emerald block
        145 => ('σ', color(68, 68, 68), None),

        _ => ('?', None, None)
    };
    if fg.is_none() {
        return BlockRender::UNKNOWN;
    }
    let fg = fg.unwrap();
    BlockRender {
        character,
        fg,
        bg
    }
}
