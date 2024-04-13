use axum::extract::ws::WebSocket;
use tokio::sync::{mpsc, watch};

use libaitfoaq::{events::Event, state::GameState, Error, Game};
use tokio_util::sync::CancellationToken;

pub type Out = GameState;
pub type In = Event;

pub struct State {
    game: Game,
    out_tx: watch::Sender<Out>,
    out_rx: watch::Receiver<Out>,
    in_tx: mpsc::Sender<In>,
    in_rx: mpsc::Receiver<In>,
}

#[derive(Clone)]
pub struct StateChannels {
    pub rx: watch::Receiver<Out>,
    pub tx: mpsc::Sender<In>,
}

impl State {
    pub fn new() -> Self {
        let game = libaitfoaq::Game::new();
        let (out_tx, out_rx) = watch::channel(game.get_game_state());
        let (in_tx, in_rx) = mpsc::channel(8);
        State {
            game,
            out_tx,
            out_rx,
            in_tx,
            in_rx,
        }
    }

    pub async fn process(&mut self, cancellation_token: CancellationToken) {
        loop {
            tokio::select! {
                Some(event) = self.in_rx.recv() => {dbg!(event);},
                _ = cancellation_token.cancelled() => { return; },
                else => { return; },
            }
        }
    }

    pub fn clonable_channels(&self) -> StateChannels {
        StateChannels {
            rx: self.out_rx.clone(),
            tx: self.in_tx.clone(),
        }
    }
}
