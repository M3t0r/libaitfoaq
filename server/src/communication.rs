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
    serializer: Serializer,
) {
    let mut connection_state = ConnectionState {
        is_admin: true,
        name: format!("{}", &peer_address),
    };
    let mut state = rx.borrow().clone();

    // send initial state
    if socket
        .send(Message::Text(serializer.game_state(&state, &connection_state)))
        .await
        .is_err()
    {
        tracing::error!(%connection_state.name, "socket was prematuerely closed");
        return;
    }

    loop {
        select! {
            msg = socket.recv() => {
                let msg = match msg {
                    Some(Ok(msg)) => msg,
                    Some(Err(error)) => {
                        tracing::warn!(%connection_state.name, ?error, "socket was closed with an error");
                        return;
                    },
                    None => {
                        tracing::info!(%connection_state.name, "socket was closed without close message");
                        return;
                    },
                };
                if let Some(answer) = match msg {
                    Message::Close(_) => {
                        tracing::info!(%connection_state.name, "socket was closed");
                        return;
                    },
                    Message::Ping(payload) => Some(Message::Pong(payload)),
                    Message::Pong(payload) => todo!("keep track of pongs"),
                    Message::Binary(_) => {
                        tracing::warn!(%connection_state.name, "received binary data instead of textual data");
                        None
                    },
                    Message::Text(msg) => {
                        match serde_json::from_str::<Input>(&msg) {
                            Err(error) => {
                                tracing::warn!(%connection_state.name, %msg, ?error, "received unrecognized msg from client");
                                None
                            },
                            Ok(input) => {
                                tracing::trace!(%connection_state.name, ?input, "received msg from client");
                                match handle_input(input).await {
                                    Ok(Some(event)) => {
                                        State::send(event, &tx).await.err().map(|e| Message::Text(serializer.error(e.into())))
                                    },
                                    Ok(None) => None,
                                    Err(error) => {
                                        tracing::error!(%connection_state.name, ?error, "encountered error while handling input");
                                        Some(Message::Text(serializer.error(error)))
                                    },
                                }
                            },
                        }
                    },
                } {
                    if let Err(error) = socket.send(answer).await {
                        tracing::warn!(?error, %connection_state.name, "failed to send back answer message");
                        return;
                    }
                }
            },
            Ok(_) = rx.changed() => {
                state = rx.borrow_and_update().clone();
                if let Err(error) = socket.send(Message::Text(serializer.game_state(&state, &connection_state))).await {
                    tracing::warn!(?error, %connection_state.name, "failed to send state update");
                    return;
                }
            }
            // todo: send ping messages in an interval
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ConnectionState {
    is_admin: bool,
    name: String,
}

#[derive(Template, serde::Serialize, serde::Deserialize)]
#[template(path = "state.html")]
struct StateTemplate {
    game: GameState,
    connection: ConnectionState,
}

#[derive(Debug)]
pub enum Serializer {
    HTML,
    JSON,
}

impl Serializer {
    #[tracing::instrument]
    fn game_state(&self, game: &GameState, connection: &ConnectionState) -> String {
        let state = StateTemplate {
            game: game.clone(),
            connection: connection.clone(),
        };
        match self {
            Self::HTML => {
                state.render().unwrap_or_else(|e| self.error(e.into()))
            },
            Self::JSON => {
                serde_json::to_string(&state).unwrap_or_else(|e| self.error(e.into()))
            },
        }
    }
    #[tracing::instrument]
    fn error(&self, error: Error) -> String {
        match self {
            Self::HTML => {
                error.render().unwrap_or("unrenderable error".to_string())
            },
            Self::JSON => {
                serde_json::json!({
                    "error": format!("{:?}", error)
                }).to_string()
            },
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case", tag = "type")]
enum Input {
    LoadBoard{board: String},
    OpenLobby,
    StartGame,
}

async fn handle_input(input: Input) -> Result<Option<libaitfoaq::events::Event>, Error> {
    match input {
        Input::LoadBoard{board: board_path} => {
            // todo: load from uploaded json or zipfile instead of path
            let board = tokio::fs::read(board_path).await?;
            let board: libaitfoaq::state::Board = serde_json::from_slice(&board)?;
            Ok(Some(Event::LoadBoard(board)))
        }
        Input::OpenLobby => Ok(Some(Event::OpenLobby)),
        Input::StartGame => Ok(Some(Event::StartGame)),
    }
}

#[derive(Debug, Error, Template)]
#[template(path = "error.html")]
enum Error {
    IO(#[from] std::io::Error),
    Parsing(#[from] serde_json::Error),
    Rendering(#[from] askama::Error),
    Game(libaitfoaq::Error),
}
impl From<libaitfoaq::Error> for Error {
    fn from(other: libaitfoaq::Error) -> Self { Self::Game(other) }
}
