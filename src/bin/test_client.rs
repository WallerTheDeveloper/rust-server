use prost::Message;
use rust_server::config::SERVER_ADDR;
use rust_server::protocol::client::{
    client_message::Payload, ClientMessage, JoinRoom, Ping, Reconnect
};
use rust_server::protocol::server::{server_message, ServerMessage};
use std::io::Error;
use std::net::UdpSocket;
use std::thread;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind("127.0.0.1:0")?;
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;

    let mut send_seq: u32 = 0;

    let _ = send_join_room(&socket, SERVER_ADDR, &mut send_seq);

    let reconnect_token = receive_and_extract_token(&socket);
    println!("Got reconnect token: {}", reconnect_token);

    // Test packet loss detection
    test_packet_loss_detection(&socket, SERVER_ADDR);

    send_seq = 110;
    
    for i in 1..=3 {
        send_ping(&socket, SERVER_ADDR, i, &mut send_seq);
        thread::sleep(Duration::from_millis(100));
    }

    println!("\n--- Simulating disconnect (waiting 35 seconds) ---");
    println!("Server timeout is 30 seconds, so we'll be marked as disconnected...");
    thread::sleep(Duration::from_secs(35));


    println!("\n--- Attempting reconnect ---");
    let new_socket = UdpSocket::bind("127.0.0.1:0")?;
    new_socket.set_read_timeout(Some(Duration::from_secs(2)))?;

    let reconnect_msg = ClientMessage {
        sequence: next_seq(&mut send_seq),
        payload: Some(Payload::Reconnect(Reconnect {
            token: reconnect_token,
            player_name: "Player1_Reconnected".to_string(),
        })),
    };
    new_socket.send_to(&reconnect_msg.encode_to_vec(), SERVER_ADDR)?;
    println!("Sent: Reconnect");
    receive_response(&new_socket);

    println!("\n--- Pings after reconnect ---");
    for i in 10..13 {
        send_ping(&new_socket, SERVER_ADDR, i, &mut send_seq);
        thread::sleep(Duration::from_secs(1));
    }

    println!("Done sending messages!");
    Ok(())
}

fn test_packet_loss_detection(socket: &UdpSocket, server_addr: &str) {
    println!("\n--- Testing packet loss detection ---");

    // Send ping with sequence 100
    let msg1 = ClientMessage {
        sequence: 100,
        payload: Some(Payload::Ping(Ping {
            timestamp: current_timestamp_ms(),
            sequence: 1,
        })),
    };
    socket.send_to(&msg1.encode_to_vec(), server_addr).unwrap();
    println!("Sent: Ping with client sequence 100");
    receive_response(socket);

    thread::sleep(Duration::from_millis(100));

    // Send ping with sequence 105 (gap of 4)
    let msg2 = ClientMessage {
        sequence: 105,
        payload: Some(Payload::Ping(Ping {
            timestamp: current_timestamp_ms(),
            sequence: 2,
        })),
    };
    socket.send_to(&msg2.encode_to_vec(), server_addr).unwrap();
    println!("Sent: Ping with client sequence 105 (skipped 101-104)");
    receive_response(socket);

    thread::sleep(Duration::from_millis(100));

    // Send ping with sequence 103 (duplicate/old)
    let msg3 = ClientMessage {
        sequence: 103,
        payload: Some(Payload::Ping(Ping {
            timestamp: current_timestamp_ms(),
            sequence: 3,
        })),
    };
    socket.send_to(&msg3.encode_to_vec(), server_addr).unwrap();
    println!("Sent: Ping with client sequence 103 (old/duplicate)");

    // This might timeout since server skips duplicates
    receive_response(socket);
}

// fn send_game_message(socket: UdpSocket, server_addr: &str) -> Result<(), Error> {
//     for i in 0..3 {
//         let game_msg = ClientMessage {
//             payload: Some(Payload::GameMessage(GameMessage {
//                 payload: format!("Game data {}", i).into_bytes(),
//             })),
//         };
//
//         socket.send_to(&game_msg.encode_to_vec(), server_addr)?;
//         println!("Sent: GameMessage {}", i);
//         thread::sleep(Duration::from_millis(100));
//     }
//     Ok(())
// }

// fn send_ready_message(socket: &UdpSocket, server_addr: &str) -> Result<(), Error> {
//     let ready_msg = ClientMessage {
//         payload: Some(Payload::Ready(Ready {})),
//     };
//     socket.send_to(&ready_msg.encode_to_vec(), server_addr)?;
//     println!("Sent: Ready");
//
//     receive_response(&socket);
//     thread::sleep(Duration::from_millis(100));
//
//     Ok(())
// }

fn send_join_room(socket: &UdpSocket, server_addr: &str, seq: &mut u32) -> Result<(), Error> {
    let join_msg = ClientMessage {
        sequence: next_seq(seq),
        payload: Some(Payload::JoinRoom(JoinRoom {
            room_code: "TEST".to_string(),
            player_name: "Player1".to_string(),
        })),
    };
    socket.send_to(&join_msg.encode_to_vec(), server_addr)?;
    println!("Sent: JoinRoom");

    // receive_response(&socket);
    thread::sleep(Duration::from_millis(100));

    Ok(())
}

fn send_ping(socket: &UdpSocket, server_addr: &str, sequence: u32, seq: &mut u32) {
    let ping_timestamp = current_timestamp_ms();

    let ping_message = ClientMessage {
        sequence: next_seq(seq),
        payload: Some(Payload::Ping(Ping {
            timestamp: ping_timestamp,
            sequence: sequence,
        })),
    };

    socket
        .send_to(&ping_message.encode_to_vec(), server_addr)
        .unwrap();
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

fn receive_and_extract_token(socket: &UdpSocket) -> String {
    let mut buf = [0u8; 1024];
    match socket.recv_from(&mut buf) {
        Ok((len, _)) => {
            if let Ok(response) = ServerMessage::decode(&buf[..len]) {
                println!("Received: {:?}", response);
                if let Some(server_message::Payload::RoomJoined(joined)) = response.payload {
                    return joined.reconnect_token;
                }
            }
        }
        Err(e) => println!("No response: {}", e),
    }
    String::new()
}

fn next_seq(seq: &mut u32) -> u32 {
    *seq += 1;
    *seq
}