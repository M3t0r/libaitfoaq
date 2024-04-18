use tokio::sync::{mpsc, watch, oneshot};
use tokio_util::sync::CancellationToken;
use thiserror::Error;

use libaitfoaq::{events::Event, state::GameState, Error as GameError, Game};

pub type Out = GameState;
pub struct In (Event, oneshot::Sender<GameError>);

#[derive(Debug)]
pub struct State {
    game: Game,
    out_tx: watch::Sender<Out>,
    out_rx: watch::Receiver<Out>,
    in_tx: mpsc::Sender<In>,
    in_rx: mpsc::Receiver<In>,
}

#[derive(Clone, Debug)]
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
        };
        Ok(state)
    }

    pub async fn send(event: Event, sender: &mpsc::Sender<In>) -> Result<(), GameError> {
        let (response_tx, response_rx) = oneshot::channel();
        sender.send(In(event, response_tx)).await.expect("could not send message to internal state processor");
        if let Ok(response) = response_rx.await {
            return Err(response);
        }
        Ok(())
    }

    pub async fn process(&mut self, cancellation_token: CancellationToken) {
        loop {
            tokio::select! {
                Some(In(event, response_channel)) = self.in_rx.recv() => {
                    match self.game.apply(event.clone()) {
                        Ok(new_state) => {
                            self.out_tx.send_replace(new_state);
                        },
                        Err(error) => {
                            let _ = response_channel.send(error);
                        },
                    }
                },
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
