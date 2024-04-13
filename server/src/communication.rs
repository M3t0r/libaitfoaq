use crate::state::StateChannels;
use axum::extract::ws::WebSocket;

pub async fn player_handler(mut socket: WebSocket, channels: StateChannels) {
    if socket.send("<h1>Connected!</h1>".into()).await.is_err() {
        // client disconnected
        return;
    }
    println!("test!");

    while let Some(msg) = socket.recv().await {
        // let msg = if let Ok(msg) = msg {
        //     msg
        // } else {
        //     // client disconnected
        //     return;
        // };
    }
}
