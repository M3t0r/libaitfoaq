pub mod events;
pub mod state;

use events::Event;
use state::*;

#[derive(Debug)]
pub struct Game {
    phase: GamePhase,
    board: Board,
    contestants: Vec<Contestant>,
}

impl Game {
    pub fn new() -> Self {
        Self {
            phase: GamePhase::Preparing,
            board: Board {
                categories: Vec::new(),
            },
            contestants: Vec::with_capacity(4),
        }
    }

    pub fn apply(&mut self, event: Event) -> Result<GameState, Error> {
        match event {
            Event::LoadBoard(board) => self.load_board(board)?,
            Event::OpenLobby => self.open_lobby()?,
            Event::ConnectContestant { name_hint } => self.connect_contestant(name_hint)?,
            Event::NameContestant { index, name } => self.name_contestant(index, name)?,
            Event::StartGame => self.start_game()?,
            _ => todo!(),
        }
        Ok(self.build_game_state())
    }

    fn build_game_state(&self) -> GameState {
        GameState {
            contestants: self.contestants.clone(),
            indicated_contestants: self
                .contestants
                .iter()
                .enumerate()
                .filter(|(_, c)| c.indicate)
                .map(|(i, _)| i)
                .collect(),
            board: self.board.clone(),
            phase: self.phase.clone(),
        }
    }

    fn load_board(&mut self, board: Board) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Preparing) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
                allowed: vec![GamePhase::Preparing],
            });
        }
        self.board = board;
        Ok(())
    }

    fn open_lobby(&mut self) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Preparing) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
                allowed: vec![GamePhase::Preparing],
            });
        }
        self.phase = GamePhase::Connecting;
        Ok(())
    }

    fn connect_contestant(&mut self, hint: String) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Connecting) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
                allowed: vec![GamePhase::Connecting],
            });
        }
        self.contestants.push(Contestant {
            name: None,
            name_hint: hint,
            points: 0 as Points,
            indicate: false,
            connected: true,
        });
        Ok(())
    }

    fn name_contestant(&mut self, index: usize, name: String) -> Result<(), Error> {
        self.contestants
            .get_mut(index)
            .ok_or(Error::ContestantNotFound)?
            .name = Some(name);
        Ok(())
    }

    fn start_game(&mut self) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Connecting) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
                allowed: vec![GamePhase::Connecting],
            });
        }
        self.phase = GamePhase::Picking;
        Ok(())
    }
}

impl Default for Game {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug)]
pub enum Error {
    WrongPhase {
        is: GamePhase,
        allowed: Vec<GamePhase>,
    },
    ContestantNotFound,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_test_board(cs: usize, qs: usize) -> Board {
        Board {
            categories: (1..cs)
                .into_iter()
                .map(|c| Category {
                    title: format!("Category {}", c),
                    questions: (1..qs)
                        .into_iter()
                        .map(|q| Question {
                            question: format!("Question {}", q),
                            answer: format!("Question {}", q),
                            hint: format!("Question {}", q),
                            points: 100 * q as Points,
                            can_wager: q == 4 && c == 2,
                            exclusive: q == 4 && c == 2,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn it_works() {
        let g = Game::default();
        let r = vec![
            Event::LoadBoard(get_test_board(6, 5)),
            Event::OpenLobby,
            Event::ConnectContestant {
                name_hint: "test_contestant_hint".to_owned(),
            },
            Event::NameContestant {
                index: 0,
                name: "Test Contestant".to_owned(),
            },
            Event::StartGame,
        ]
        .into_iter()
        .fold(g, |mut g, e| {
            g.apply(e).expect("could not apply event");
            g
        });
        assert_eq!(r.board, get_test_board(6, 5));
        assert_eq!(r.contestants.len(), 1);
        assert_eq!(r.contestants[0].name, Some("Test Contestant".to_owned()));
        assert!(matches!(r.phase, GamePhase::Picking));
    }
}
