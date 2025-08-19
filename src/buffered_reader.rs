use std::error::Error;
use std::io::prelude::*;
use std::net::TcpStream;
use openssl::symm::{Cipher, Mode, Crypter};
use tokio::net::tcp::OwnedReadHalf;
use tokio::io::AsyncReadExt;

use crate::log;

const PACKET_SIZE: usize = 65535;

/// Provides a contiguous block of data of requested size from a TCP stream
pub struct BufferedReader {
    buffer: [u8; PACKET_SIZE*2],
    encrypted_buffer: [u8; PACKET_SIZE*2],
    stream: Option<TcpStream>,
    reader: Option<OwnedReadHalf>,
    pos: usize,
    encrypted_pos: usize,
    available: usize,
    encrypted_available: usize,
    log_enable: bool,
    decrypt_enable: bool,
    decrypter: Option<Crypter>,
}

impl BufferedReader {
    fn new() -> Self {
        BufferedReader {
            buffer: [0u8; PACKET_SIZE*2],
            encrypted_buffer: [0u8; PACKET_SIZE*2],
            pos: 0,
            encrypted_pos: 0,
            available: 0,
            encrypted_available: 0,
            log_enable: false,
            decrypt_enable: false,
            decrypter: None,
            stream: None,
            reader: None
        }
    }

    async fn try_read(&mut self) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if let Some(reader) = self.reader.as_mut() {
            match reader.read(&mut self.buffer[self.pos+self.available..]).await {
                Ok(n) => Ok(n),
                Err(err) => Err(Box::new(err))
            }
        } else {
            Ok(self.stream.as_ref().unwrap()
                .read(&mut self.buffer[self.pos+self.available..]).map_err(|e| e.to_string())?)
        }
    }

    async fn read(&mut self) -> Result<usize, Box<dyn Error + Send + Sync>> {
        if self.decrypt_enable {
            let ret = self.try_read().await?;
            let mut temp = vec![0; ret];
            self.decrypter.as_mut().unwrap().update(
                &self.buffer[self.pos+self.available..self.pos+self.available+ret],
                &mut temp[..])?;
            for i in 0..ret {
                self.buffer[self.pos+self.available+i] = temp[i];
            }
            Ok(ret)
        } else {
            self.try_read().await
        }
    }

    pub fn from_stream(stream: TcpStream) -> Self {
        let mut reader = BufferedReader::new();
        reader.stream = Some(stream);
        reader
    }

    pub fn from_reader(read: OwnedReadHalf) -> Self {
        let mut reader = BufferedReader::new();
        reader.reader = Some(read);
        reader
    }

    pub fn enable_decryption(&mut self){
        self.decrypt_enable = true;
    }
    pub fn set_decryption_key(&mut self, key: &[u8]) -> Result<(), Box<dyn Error + Send + Sync>>  {
        if let Ok(crypter) = Crypter::new(Cipher::aes_128_cfb8(), Mode::Decrypt, key, Some(key)) {
            self.decrypter = Some(crypter);
            Ok(())
        } else {
            Err("Failed to create decrypter".into())
        }
    }

    fn compact_encrypted(&mut self) {
        if self.log_enable { log::trace!("[BufRead] Moving encrypted buffer pointer from {} to 0", self.encrypted_pos); }
        if self.encrypted_available > 0 {
            if self.log_enable { log::trace!("[BufRead] compacting {} encrypted bytes", self.encrypted_available); }
            self.encrypted_buffer.copy_within(self.encrypted_pos..self.encrypted_available+self.encrypted_pos, 0);
        }
        self.encrypted_pos = 0;
    }

    fn compact_buffer(&mut self) {
        if self.log_enable { log::trace!("[BufRead] Moving buffer pointer from {} to 0", self.pos); }
        if self.available > 0 {
            if self.log_enable { log::trace!("[BufRead] compacting {} bytes", self.available); }
            self.buffer.copy_within(self.pos..self.available+self.pos, 0);
        }
        self.pos = 0;
    }

    pub async fn read_bytes(&mut self, count: usize) -> Result<&[u8], Box<dyn Error + Send + Sync>> {
        if self.log_enable { log::trace!("[BufRead] requested {} bytes, have {}", count, self.available); }
        if count >= PACKET_SIZE {
            return Err(format!("Requested more than you can chew: {count}").into());
        }
        loop {
            if self.available >= count {
                self.available -= count;
                let slice = &self.buffer[self.pos..count+self.pos];
                if self.log_enable { log::trace!("Returning slice of {}..{}  data: {:02X?}",self.pos, count, slice); }
                self.pos += count;
                return Ok(slice)
            }
            // Looks like there's not enough data available
            // Check whether we can fit the rest into the buffer
            if count + self.available + self.pos > PACKET_SIZE {
                self.compact_buffer();
            }
            if self.encrypted_available + self.encrypted_pos > PACKET_SIZE {
                self.compact_encrypted();
            }

            let red = self.read().await?;
            if self.log_enable { log::trace!("[BufRead] Received {} bytes", red); }
            if red == 0 {
                return Err("End of stream".into())
            }
            self.available += red;
        }
    }

    pub async fn read_string(&mut self) -> Result<String, Box<dyn Error + Send + Sync>> {
        let length = self.read_ushort().await? as usize;
        if length == 0 {
            return Ok(String::new());
        }
        let data = self.read_bytes(length*2).await?;
        let mut codepoints = vec![0u16; length];
        for i in 0..length {
            let indx = i*2;
            codepoints[i] = to_ushort(&data[indx..indx+2]);
        }
        std::string::String::from_utf16(codepoints.as_slice()).map_err(|e| e.to_string().into())
    }

    pub async fn read_bool(&mut self) -> Result<bool, Box<dyn Error + Send + Sync>> {
        let data = self.read_bytes(1).await?[0];
        if data > 0x01 {
            if self.log_enable { log::trace!("[BufRead] Warning: bool field was {}", data); }
        }
        Ok(data == 0x01)
    }

    pub async fn read_ubyte(&mut self) -> Result<u8, Box<dyn Error + Send + Sync>> {
        Ok(self.read_bytes(1).await?[0])
    }

    pub async fn read_byte(&mut self) -> Result<i8, Box<dyn Error + Send + Sync>> {
        Ok(self.read_bytes(1).await?[0] as i8)
    }

    pub async fn read_long(&mut self) -> Result<i64, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_bytes(8).await?);
        Ok(i64::from_be_bytes(bytes))
    }

    pub async fn read_float(&mut self) -> Result<f32, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.read_bytes(4).await?);
        Ok(f32::from_be_bytes(bytes))
    }

    pub async fn read_double(&mut self) -> Result<f64, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(self.read_bytes(8).await?);
        Ok(f64::from_be_bytes(bytes))
    }

    pub async fn read_int(&mut self) -> Result<i32, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 4];
        bytes.copy_from_slice(self.read_bytes(4).await?);
        Ok(i32::from_be_bytes(bytes))
    }

    pub async fn read_short(&mut self) -> Result<i16, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.read_bytes(2).await?);
        Ok(i16::from_be_bytes(bytes))
    }

    pub async fn read_ushort(&mut self) -> Result<u16, Box<dyn Error + Send + Sync>> {
        let mut bytes = [0u8; 2];
        bytes.copy_from_slice(self.read_bytes(2).await?);
        Ok(u16::from_be_bytes(bytes))
    }
}

fn to_ushort(data: &[u8]) -> u16 {
    let mut bytes = [0u8; 2];
    bytes.copy_from_slice(data);
    u16::from_be_bytes(bytes)
}
