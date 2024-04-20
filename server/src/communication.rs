use std::net::SocketAddr;

use crate::state::{State, StateChannels};
use axum::extract::ws::{Message, WebSocket};
use askama::Template;
use libaitfoaq::{events::Event, state::{ContestantHandle, GameState}};
use tokio::select;
use serde::Deserialize;
use thiserror::Error;

#[tracing::instrument(skip(socket, rx, tx))]
pub async fn player_handler(
    socket: WebSocket,
    peer_address: SocketAddr,
    StateChannels{rx, tx}: StateChannels,
    serializer: Serializer,
) {
    let mut connection = Connection {
        should_disconnect: false,
        socket,
        tx,
        rx,
        serializer,
        state: ConnectionState {
            is_admin: true,
            name: format!("{}", &peer_address),
            controlling: None,
        },
    };
    let state = connection.rx.borrow().clone();

    // send initial state
    if connection.socket
        .send(Message::Text(connection.serializer.game_state(&state, &connection.state)))
        .await
        .is_err()
    {
        tracing::error!(%connection.state.name, "socket was prematuerely closed");
        return;
    }

    // put his whole thing in a struct and put the body of the loop in a function
    // implement Drop to send Event::DisconnectPlayer on connection close

    while !connection.should_disconnect {
        select! {
            msg = connection.socket.recv() => { connection.handle_message(msg).await; },
            Ok(_) = connection.rx.changed() => { connection.handle_new_game_state().await; }
            // todo: send ping messages in an interval
        }
    }

    if let Some(c) = connection.state.controlling {
        let _ = State::send(Event::DisconnectContestant { contestant: c }, &connection.tx).await;
    }
}

struct Connection {
    should_disconnect: bool,
    socket: WebSocket,
    tx: tokio::sync::mpsc::Sender<crate::state::In>,
    rx: tokio::sync::watch::Receiver<crate::state::Out>,
    serializer: Serializer,
    state: ConnectionState,
}

impl Connection {
    async fn handle_new_game_state(&mut self) {
        let new = self.rx.borrow_and_update().clone();
        if let Err(error) = self.socket.send(
            Message::Text(self.serializer.game_state(&new, &self.state))
        ).await {
            self.disconnect(error.into(), "failed to send state update").await;
        }
    }
    async fn handle_message(&mut self, msg: Option<Result<Message, axum::Error>>) {
        let msg = match msg {
            Some(Ok(msg)) => msg,
            Some(Err(error)) => {
                return self.disconnect(error.into(), "socket was closed with an error").await;
            },
            None => {
                return self.disconnect_without_error("socket was closed without close message").await;
            },
        };
        match msg {
            Message::Close(_) => {
                return self.disconnect_without_error("socket was closed").await;
            },
            Message::Ping(payload) => {
                // tracing::trace!(%self.state.name, "received ping");
                self.send_msg(Message::Pong(payload)).await;
            },
            Message::Pong(payload) => todo!("keep track of pongs"),
            Message::Binary(_) => {
                tracing::warn!(%self.state.name, "received binary data instead of textual data");
                return;
            },
            Message::Text(msg) => {
                match serde_json::from_str::<Input>(&msg) {
                    Err(error) => {
                        tracing::warn!(%self.state.name, %msg, ?error, "received unrecognized msg from client");
                    },
                    Ok(input) => {
                        tracing::trace!(%self.state.name, ?input, "received msg from client");
                        match handle_input(input).await {
                            Ok(Some(Event::ConnectContestant { name_hint })) => {
                                if self.state.controlling.is_some() { return };
                                let event = Event::ConnectContestant { name_hint };
                                match State::send(event, &self.tx).await {
                                    Err(e) => { self.send_error(e.into()).await; }
                                    Ok(state) => {
                                        self.state.controlling = Some(state.contestants.len());
                                    },
                                }
                            },
                            Ok(Some(Event::DisconnectContestant { contestant })) => {
                                if self.state.controlling.is_none() { return };
                                let event = Event::DisconnectContestant { contestant };
                                match State::send(event, &self.tx).await {
                                    Err(e) => { self.send_error(e.into()).await; }
                                    Ok(_) => {
                                        self.state.controlling = None;
                                    },
                                }
                            },
                            Ok(Some(Event::ReconnectContestant { contestant })) => {
                                if self.state.controlling.is_some() { return };
                                let event = Event::ReconnectContestant { contestant };
                                match State::send(event, &self.tx).await {
                                    Err(e) => { self.send_error(e.into()).await; }
                                    Ok(_) => {
                                        self.state.controlling = Some(contestant);
                                    },
                                }
                            },
                            Ok(Some(event)) => {
                                if let Err(e) = State::send(event, &self.tx).await {
                                    self.send_error(e.into()).await;
                                }
                            },
                            Ok(None) => {},
                            Err(error) => {
                                tracing::error!(%self.state.name, ?error, "encountered error while handling input");
                                self.send_error(error).await;
                            },
                        }
                    },
                }
            },
        } {
        }
    }
    async fn send_error(&mut self, err: Error) {
        self.send_msg(Message::Text(self.serializer.error(err))).await;
    }
    async fn send_state(&mut self, state: GameState) {}
    async fn send_msg(&mut self, msg: Message) {
        if let Err(error) = self.socket.send(msg).await {
            self.disconnect(error.into(), "failed to send message").await;
        }
    }
    async fn disconnect(&mut self, cause: Error, reason: &str) {
        tracing::warn!(%self.state.name, ?cause, %reason, "disconnecting");
        self.should_disconnect = true; // stop the main loop
    }
    async fn disconnect_without_error(&mut self, reason: &str) {
        tracing::info!(%self.state.name, %reason, "disconnecting");
        self.should_disconnect = true; // stop the main loop
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ConnectionState {
    is_admin: bool,
    name: String,
    controlling: Option<ContestantHandle>,
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
    ConnectContestant { name_hint: String },
    ReconnectContestant { contestant: ContestantHandle },
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
        Input::ConnectContestant { name_hint } => Ok(Some(Event::ConnectContestant { name_hint })),
        Input::ReconnectContestant { contestant } => Ok(Some(Event::ReconnectContestant { contestant })),
    }
}

#[derive(Debug, Error, Template)]
#[template(path = "error.html")]
enum Error {
    IO(#[from] std::io::Error),
    Network(#[from] axum::Error),
    Parsing(#[from] serde_json::Error),
    Rendering(#[from] askama::Error),
    Game(libaitfoaq::Error),
}
impl From<libaitfoaq::Error> for Error {
    fn from(other: libaitfoaq::Error) -> Self { Self::Game(other) }
}
