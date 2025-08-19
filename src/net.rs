use std::error::Error;

use tokio::sync::mpsc::Receiver;
use tokio::sync::Mutex;
use tokio::net::{TcpStream, tcp::OwnedWriteHalf};
use tokio::io::AsyncWriteExt;
use crate::packets::{Packet, write, try_read, read};
use crate::log;
use crate::buffered_reader::BufferedReader;

use openssl::rsa::{Rsa, Padding};
use openssl::symm::{Cipher, Mode, Crypter};
use openssl::rand::rand_bytes;

pub struct Connection {
    inbound: Receiver<Packet>,
    write: Mutex<OwnedWriteHalf>,
    encryption: bool,
    encrypter: Option<Mutex<Crypter>>,
    sender_loop: Option<tokio::task::JoinHandle<()>>
}

impl Connection {
    pub async fn send(&self, packet: Packet) -> Result<(), Box<dyn Error>>{
        let raw_packet = if self.encryption {
            let unencrypted = write(packet);
            let mut encrypted = vec![0; unencrypted.len()];
            self.encrypter.as_ref().unwrap().lock().await.update(
                &unencrypted,
                &mut encrypted)?;
            encrypted
        } else {
            write(packet)
        };
        let mut tries = 0;
        let mut bytes_sent = 0;
        while bytes_sent != raw_packet.len() || tries < 5 {
            let previous_sent = bytes_sent;
            bytes_sent += self.write.lock().await.write(&raw_packet[bytes_sent..]).await?;
            if bytes_sent == previous_sent {
                tries += 1;
            } else {
                tries = 0;
            }
        }
        if tries > 5 {
            Err("Failed to write packet after N attempts".into())
        } else {
            Ok(())
        }
    }

    pub async fn recv(&mut self, buffer: &mut Vec<Packet>) {
        if !self.inbound.is_empty() {
            self.inbound.recv_many(buffer, 1000).await;
        }
    }

    async fn enable_encryption(
        &mut self,
        pbkey: &[u8],
        verify_token: &[u8],
        reader: &mut BufferedReader)
        -> Result<(), Box<dyn Error>>
    {
        if self.encryption {
            log::warning!("Encryption already enabled");
            return Ok(())
        }

        let rsa = Rsa::public_key_from_der(pbkey).unwrap();

        let mut shared: [u8; 16] = [0; 16];
        rand_bytes(&mut shared).unwrap();
        let mut shared_out: [u8; 128] = [0; 128];
        rsa.public_encrypt(
            &shared,
            &mut shared_out,
            Padding::PKCS1).unwrap();
        let mut token_out: [u8; 128] = [0; 128];
        rsa.public_encrypt(
            verify_token,
            &mut token_out,
            Padding::PKCS1).unwrap();

        reader.set_decryption_key(&shared)
            .map_err(|e| format!("Failed to enable decrpytion: {}", e)).unwrap();
        self.encrypter = Some(Mutex::new(Crypter::new(
            Cipher::aes_128_cfb8(),
            Mode::Encrypt,
            &shared,
            Some(&shared)).unwrap()));
        self.send(Packet::EncryptionKeyResponse {
            shared_secret: Box::from(shared_out),
            verify_token: Box::from(token_out)
        }).await?;
        self.encryption = true;
        Ok(())
    }

    pub async fn connect_offline(host: &str, port: i32, username: &str) -> Result<Connection, Box<dyn Error>> {
        let address = format!("{}:{}", host, port);
        let (reader, writer) = TcpStream::connect(&address).await?.into_split();
        log::info!("Connected to {}", address);
        let( tx, rx ) = tokio::sync::mpsc::channel::<Packet>(1000);
        let mut connection = Connection {
            inbound: rx, 
            write: Mutex::new(writer),
            encryption: false,
            encrypter: None,
            sender_loop: None
        };

        connection.send(Packet::Handshake {
            protocol_version: 61,
            host: host.to_owned(),
            username: username.to_owned(),
            port: port
        }).await?;

        let mut buf_reader = BufferedReader::from_reader(reader);

        if let Packet::EncryptionKeyRequest{pbkey, verify_token, ..} = read(&mut buf_reader).await {
            connection.enable_encryption(pbkey.as_ref(), verify_token.as_ref(), &mut buf_reader).await?;
        } else {
            return Err("Wrong packet after handshake - expected EncryptionKeyRequest".into())
        }

        if let Packet::EncryptionKeyResponse{shared_secret, verify_token} = read(&mut buf_reader).await {
            if shared_secret.len() != 0 || verify_token.len() != 0 {
                log::warning!("EncryptionKeyRespons wasn't empty - is something wrong?");
            }
            buf_reader.enable_decryption();
            connection.send(Packet::ClientStatuses {payload: 0}).await?;
        } else {
            return Err("Wrong packet after handshake - expected empty EncryptionKeyResponse".into())
        };

        connection.sender_loop = Some(tokio::task::spawn( async move {
            loop {
                match try_read(&mut buf_reader).await {
                    Err(e) => {
                        log::error!("Error reading packet, exiting: {}", e);
                        break;
                    },
                    Ok(packet) => {
                        if let Err(_) = tx.send(packet).await {
                            log::error!("Error in receiver loop, channel closed!");
                            break;
                        }
                    }
                }
            }
        }));
        Ok(connection)
    }
}

