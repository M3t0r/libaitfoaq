use std::net::SocketAddr;

use crate::state::{State, StateChannels};
use axum::extract::ws::{Message, WebSocket};
use askama::Template;
use libaitfoaq::{events::Event, state::GameState};
use tokio::select;
use serde::Deserialize;
use thiserror::Error;

#[tracing::instrument(skip(socket, rx, tx))]
pub async fn player_handler(
    mut socket: WebSocket,
    peer_address: SocketAddr,
    StateChannels{mut rx, tx}: StateChannels,
) {
    let mut connection_name = format!("{}", &peer_address);
    let mut state = rx.borrow().clone();

    // send initial state
    if socket
        .send(Message::Text(render_game_state(&state)))
        .await
        .is_err()
    {
        tracing::error!(%connection_name, "socket was prematuerely closed");
        return;
    }

    loop {
        select! {
            msg = socket.recv() => {
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(error)) => {
                        tracing::warn!(%connection_name, ?error, "socket was closed with an error");
                        return;
                    },
                    None => {
                        tracing::info!(%connection_name, "socket was closed without close message");
                        return;
                    },
                };
                if let Some(answer) = match msg {
                    Message::Close(_) => {
                        tracing::info!(%connection_name, "socket was closed");
                        return;
                    },
                    Message::Ping(payload) => Some(Message::Pong(payload)),
                    Message::Pong(payload) => todo!("keep track of pongs"),
                    Message::Binary(_) => {
                        tracing::warn!(%connection_name, "received binary data instead of textual data");
                        None
                    },
                    Message::Text(msg) => {
                        match serde_json::from_str::<Input>(&msg) {
                            Err(error) => {
                                tracing::warn!(%connection_name, %msg, ?error, "received unrecognized msg from client");
                                None
                            },
                            Ok(input) => {
                                tracing::trace!(%connection_name, ?input, "received msg from client");
                                match handle_input(input).await {
                                    Ok(Some(event)) => {
                                        State::send(event, &tx).await.err().map(|e| Message::Text(Error::from(e).render().unwrap_or("unrenderable error".to_string())))
                                    },
                                    Ok(None) => None,
                                    Err(error) => {
                                        tracing::error!(%connection_name, ?error, "encountered error while handling input");
                                        Some(Message::Text(error.render().unwrap_or("unrenderable error".to_string())))
                                    },
                                }
                            },
                        }
                    },
                } {
                    if let Err(error) = socket.send(answer).await {
                        tracing::warn!(?error, %connection_name, "failed to send back answer message");
                        return;
                    }
                }
            },
            Ok(_) = rx.changed() => {
                state = rx.borrow_and_update().clone();
                if let Err(error) = socket.send(Message::Text(render_game_state(&state))).await {
                    tracing::warn!(?error, %connection_name, "failed to send state update");
                    return;
                }
            }
            // todo: send ping messages in an interval
        }
    }
}

#[derive(Template)]
#[template(path = "state.html")]
struct StateTemplate {
    state: GameState,
    is_admin: bool,
}

#[tracing::instrument]
fn render_game_state(state: &GameState) -> String {
    StateTemplate {
        state: state.clone(),
        is_admin: true,
    }.render().unwrap()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Input {
    LoadBoard{board: String},
}

async fn handle_input(input: Input) -> Result<Option<libaitfoaq::events::Event>, Error> {
    match input {
        Input::LoadBoard{board: board_path} => {
            let board = tokio::fs::read(board_path).await?;
            let board: libaitfoaq::state::Board = serde_json::from_slice(&board)?;
            return Ok(Some(Event::LoadBoard(board)));
        }
    }
    Ok(None)
}

#[derive(Debug, Error, Template)]
#[template(path = "error.html")]
enum Error {
    IO(#[from] std::io::Error),
    Parsing(#[from] serde_json::Error),
    Game(libaitfoaq::Error),
}
impl From<libaitfoaq::Error> for Error {
    fn from(other: libaitfoaq::Error) -> Self { Self::Game(other) }
}
