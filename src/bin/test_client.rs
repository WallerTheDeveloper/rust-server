use prost::Message;
use std::net::UdpSocket;
use std::time::Duration;
use std::thread;
use rust_server::protocol::client::{
    ClientMessage, JoinRoom, Ready, GameMessage,
    client_message::Payload,
};
use rust_server::protocol::server::ServerMessage;

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    let server_addr = "127.0.0.1:9000";

    // 1. Join room
    let join_msg = ClientMessage {
        payload: Some(Payload::JoinRoom(JoinRoom {
            room_code: "TEST".to_string(),
            player_name: "Player1".to_string(),
        })),
    };
    socket.send_to(&join_msg.encode_to_vec(), server_addr)?;
    println!("Sent: JoinRoom");

    receive_response(&socket);
    thread::sleep(Duration::from_millis(100));

    // 2. Send Ready
    let ready_msg = ClientMessage {
        payload: Some(Payload::Ready(Ready {})),
    };
    socket.send_to(&ready_msg.encode_to_vec(), server_addr)?;
    println!("Sent: Ready");

    receive_response(&socket);
    thread::sleep(Duration::from_millis(100));

    // 3. Send some game messages (opaque data)
    for i in 0..3 {
        let game_msg = ClientMessage {
            payload: Some(Payload::GameMessage(GameMessage {
                payload: format!("Game data {}", i).into_bytes(),
            })),
        };
        socket.send_to(&game_msg.encode_to_vec(), server_addr)?;
        println!("Sent: GameMessage {}", i);
        thread::sleep(Duration::from_millis(100));
    }

    println!("Done!");
    Ok(())
}

fn receive_response(socket: &UdpSocket) {
    let mut buf = [0u8; 1024];
    match socket.recv_from(&mut buf) {
        Ok((len, _)) => {
            if let Ok(response) = ServerMessage::decode(&buf[..len]) {
                println!("Received: {:?}", response);
            } else {
                println!("Received {} bytes (failed to decode)", len);
            }
        }
        Err(e) => println!("No response: {}", e),
    }
}