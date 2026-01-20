use std::io::Error;
use prost::Message;
use std::net::UdpSocket;
use std::time::Duration;
use std::thread;
use rust_server::protocol::client::{
    ClientMessage, JoinRoom, Ready, GameMessage, Ping,
    client_message::Payload,
};

use rust_server::protocol::server::{ServerMessage};

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    let server_addr = "127.0.0.1:9000";

    // 1. Send ping
    println!("--Sending ping to server before joining--");
    send_ping(&socket, server_addr, 0);

    // 2. Join room
    send_join_room_message(&socket, server_addr).expect("Join room message: success");

    // 3. Multiple pings after joining
    for i in 1..=5 {
        send_ping(&socket, server_addr, i);
        thread::sleep(Duration::from_millis(100));
    }

    // 4. Send Ready
    send_ready_message(&socket, server_addr).expect("Read message: success");

    // 5. Send some game messages (opaque data)
    // send_game_message(socket, server_addr);

    println!("Done sending messages!");
    Ok(())
}

fn send_game_message(socket: UdpSocket, server_addr: &str) -> Result<(), Error>{
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
    Ok(())
}

fn send_ready_message(socket: &UdpSocket, server_addr: &str) -> Result<(), Error> {
    let ready_msg = ClientMessage {
        payload: Some(Payload::Ready(Ready {})),
    };
    socket.send_to(&ready_msg.encode_to_vec(), server_addr)?;
    println!("Sent: Ready");

    receive_response(&socket);
    thread::sleep(Duration::from_millis(100));

    Ok(())
}

fn send_join_room_message(socket: &UdpSocket, server_addr: &str) -> Result<(), Error> {
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

    Ok(())
}

fn send_ping(socket: &UdpSocket, server_addr: &str, sequence: u32) {
    let ping_timestamp = current_timestamp_ms();

    let ping_message = ClientMessage {
        payload: Some(Payload::Ping(Ping{
            timestamp: ping_timestamp,
            sequence: sequence,
        })),
    };

    socket.send_to( &ping_message.encode_to_vec(), server_addr).unwrap();
    println!("Sent: Ping (seq={})", sequence);

    let mut buffer = [0u8; 1024];
    match socket.recv_from(&mut buffer) {
        Ok((len, _)) => {
            let now = current_timestamp_ms();
            if let Ok(response) = ServerMessage::decode(&buffer[..len]) {
                println!("Received response: {:?}", response);
                println!("Round trip latency: {} ms", now - ping_timestamp);
            }
        }
        Err(e) => {
            println!("No pong received: {}", e);
        }
    }
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

// TODO: I have the same function in main.rs. Remember about DRY
fn current_timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}