use prost::Message;
use std::net::UdpSocket;
use std::time::Duration;
use std::thread;
use rust_server::protocol::client::{
    ClientMessage, JoinRoom, PlayerInput, Ready,
    client_message::Payload,
};
use rust_server::protocol::server::ServerMessage;

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    let server_addr = "127.0.0.1:9000";

    // 1. Send JoinRoom
    let join_msg = ClientMessage {
        payload: Some(Payload::JoinRoom(JoinRoom {
            room_code: "ABCD".to_string(),
            player_name: "TestPlayer".to_string(),
        })),
    };
    socket.send_to(&join_msg.encode_to_vec(), server_addr)?;
    println!("Sent: JoinRoom");

    // Wait for response
    let mut buf = [0u8; 1024];
    match socket.recv_from(&mut buf) {
        Ok((len, _)) => {
            if let Ok(response) = ServerMessage::decode(&buf[..len]) {
                println!("Received: {:?}", response);
            }
        }
        Err(e) => println!("No response: {}", e),
    }

    thread::sleep(Duration::from_millis(100));

    // 2. Send Ready
    let ready_msg = ClientMessage {
        payload: Some(Payload::Ready(Ready {})),
    };
    socket.send_to(&ready_msg.encode_to_vec(), server_addr)?;
    println!("Sent: Ready");

    thread::sleep(Duration::from_millis(100));

    // 3. Send some PlayerInput
    for i in 0..3 {
        let input_msg = ClientMessage {
            payload: Some(Payload::PlayerInput(PlayerInput {
                tick: i,
                direction: (i as f32) * 0.5,
                dash: i % 2 == 0,
            })),
        };
        socket.send_to(&input_msg.encode_to_vec(), server_addr)?;
        println!("Sent: PlayerInput tick={}", i);
        thread::sleep(Duration::from_millis(100));
    }

    println!("Done!");
    Ok(())
}