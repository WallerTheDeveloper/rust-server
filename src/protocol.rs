pub mod common {
    include!(concat!(env!("OUT_DIR"), "/game.common.rs"));
}

pub mod client {
    include!(concat!(env!("OUT_DIR"), "/game.client.rs"));
}

pub mod server {
    include!(concat!(env!("OUT_DIR"), "/game.server.rs"));
}

pub mod paperio {
    include!(concat!(env!("OUT_DIR"), "/game.paperio.rs"));
}