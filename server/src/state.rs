use tokio::sync::{mpsc, watch, oneshot};
use tokio_util::sync::CancellationToken;
use thiserror::Error;

use libaitfoaq::{events::Event, state::GameState, Error as GameError, Game};

use std::path::Path;
use std::io::Write;

pub type Out = GameState;
pub struct In (Event, oneshot::Sender<Result<GameState, GameError>>);

#[derive(Debug)]
pub struct State<'a> {
    admin_token: String,
    game: Game,
    journal_path: &'a Path,
    journal_writer: std::fs::File,
    out_tx: watch::Sender<Out>,
    out_rx: watch::Receiver<Out>,
    in_tx: mpsc::Sender<In>,
    in_rx: mpsc::Receiver<In>,
}

#[derive(Clone, Debug)]
pub struct StateChannelsAndToken {
    pub admin_token: String,
    pub rx: watch::Receiver<Out>,
    pub tx: mpsc::Sender<In>,
}

impl<'a> State<'a> {
    pub fn with_journal_and_token(journal_path: &'a Path, token: String) -> Result<Self, Error> {
        let mut game = libaitfoaq::Game::new();

        if journal_path.exists() {
            let journal = std::fs::read(journal_path)
                .map_err(|e| Error::IOLoading(journal_path.to_owned(), e))?;
            for event in serde_json::Deserializer::from_slice(&journal).into_iter::<Event>() {
                let event = event.map_err(|e| Error::Parsing(journal_path.to_owned(), e))?;
                game.apply(event).map_err(|e| Error::Loading(journal_path.to_owned(), e))?;
            }
            game.mark_all_contestants_as_disconnected();
        }

        let journal_writer = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .append(true)
            .open(journal_path)
            .map_err(|e| Error::IOSaving(journal_path.to_owned(), e))?;

        let (out_tx, out_rx) = watch::channel(game.get_game_state());
        let (in_tx, in_rx) = mpsc::channel(8);
        let state = State {
            admin_token: token,
            game,
            journal_path,
            journal_writer,
            out_tx,
            out_rx,
            in_tx,
            in_rx,
        };
        Ok(state)
    }

    pub async fn send(event: Event, sender: &mpsc::Sender<In>) -> Result<GameState, GameError> {
        let (response_tx, response_rx) = oneshot::channel();
        sender.send(In(event, response_tx)).await.expect("could not send message to internal state processor");
        response_rx.await.expect("Can't receive answer from state processor")
    }

    pub async fn process(&mut self, cancellation_token: CancellationToken) {
        loop {
            tokio::select! {
                Some(In(event, response_channel)) = self.in_rx.recv() => {
                    match self.game.apply(event.clone()) {
                        Ok(new_state) => {
                            self.write_to_journal(event).await.expect("Can't write to journal");
                            let _ = response_channel.send(Ok(new_state.clone()));
                            self.out_tx.send_replace(new_state);
                        },
                        Err(error) => {
                            let _ = response_channel.send(Err(error));
                        },
                    }
                },
                _ = cancellation_token.cancelled() => { return; },
                else => { return; },
            }
        }
    }

    pub fn clonable_channels(&self) -> StateChannelsAndToken {
        StateChannelsAndToken {
            admin_token: self.admin_token.clone(),
            rx: self.out_rx.clone(),
            tx: self.in_tx.clone(),
        }
    }

    async fn write_to_journal(&mut self, event: Event) -> Result<(), Error> {
        let mut bytes = serde_json::to_vec(&event)
            .map_err(|e| Error::Saving(self.journal_path.to_owned(), e))?;
        bytes.push(0x0a); // add a newline
        self.journal_writer.write(&bytes)
            .map_err(|e| Error::IOSaving(self.journal_path.to_owned(), e))?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("Could not read the journal file: {0}: {1}")]
    IOLoading(std::path::PathBuf, std::io::Error),
    #[error("Could not write the journal file: {0}: {1}")]
    IOSaving(std::path::PathBuf, std::io::Error),
    #[error("Could not save to journal file: {0}: {1}")]
    Saving(std::path::PathBuf, serde_json::Error),
    #[error("Could not parse journal file: {0}: {1}")]
    Parsing(std::path::PathBuf, serde_json::Error),
    #[error("Could not load journal file: {0}: {1:?}")]
    Loading(std::path::PathBuf, GameError),
}
