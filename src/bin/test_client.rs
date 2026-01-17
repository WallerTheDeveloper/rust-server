use prost::Message;
use std::net::UdpSocket;
use rust_server::protocol::client::{ClientMessage, JoinRoom, client_message::Payload};

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    let server_addr = "127.0.0.1:9000";

    let msg = ClientMessage {
        payload: Some(Payload::JoinRoom(JoinRoom {
            room_code: "ABCD".to_string(),
            player_name: "TestPlayer".to_string(),
        })),
    };

    let bytes = msg.encode_to_vec();
    println!("Sending {} bytes: {:?}", bytes.len(), bytes);

    socket.send_to(&bytes, server_addr)?;
    println!("Sent JoinRoom message to {}", server_addr);

    socket.set_read_timeout(Some(std::time::Duration::from_secs(5)))?;

    let mut buf = [0u8; 1024];
    match socket.recv_from(&mut buf) {
        Ok((len, addr)) => {
            println!("Received {} bytes from {}", len, addr);
        }
        Err(e) => {
            println!("No response (expected for now): {}", e);
        }
    }

    Ok(())
}