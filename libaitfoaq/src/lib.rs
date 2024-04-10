pub mod events;
pub mod state;

#[cfg(test)]
mod test_state_machine;

use events::Event;
use state::*;

#[derive(Debug)]
pub struct Game {
    phase: GamePhase,
    board: Board,
    contestants: Vec<Contestant>,
    indications: Vec<usize>,
    options: Options,
}

impl Game {
    pub fn new() -> Self {
        Self {
            phase: GamePhase::Preparing,
            board: Board {
                categories: Vec::new(),
            },
            contestants: Vec::with_capacity(4),
            indications: Vec::with_capacity(4),
            options: Options::default(),
        }
    }

    pub fn apply(&mut self, event: Event) -> Result<GameState, Error> {
        match event {
            Event::LoadBoard(board) => self.load_board(board)?,
            Event::OpenLobby => self.open_lobby()?,
            Event::ConnectContestant { name_hint } => self.connect_contestant(name_hint)?,
            Event::NameContestant { index, name } => self.name_contestant(index, name)?,
            Event::StartGame => self.start_game()?,
            Event::Pick { category_index, clue_index } =>  self.pick(category_index, clue_index)?,
            Event::ClueFullyShown => self.clue_fully_shown()?,
            Event::Buzz { contestant_index } => self.buzz(contestant_index)?,
            Event::SetWage { points } => self.set_wage(points)?,
            Event::AcceptAnswer => self.accept_answer()?,
            Event::RejectAnswer => self.reject_answer()?,
            Event::RevealHint => self.reveal_hint()?,
            Event::FinishClue => self.finish_clue()?,
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
            options: self.options.clone(),
        }
    }

    fn load_board(&mut self, board: Board) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Preparing) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        self.board = board;
        Ok(())
    }

    fn open_lobby(&mut self) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Preparing) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        self.phase = GamePhase::Connecting;
        Ok(())
    }

    fn connect_contestant(&mut self, hint: String) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Connecting) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
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
            });
        }
        if self.contestants.is_empty() && !self.options.allow_game_without_contestant {
            return Err(Error::NoContestants);
        }
        self.phase = GamePhase::Picking;
        Ok(())
    }

    fn pick(&mut self, category_index: usize, clue_index: usize) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Picking) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        let clue = self.board
            .categories.get(category_index).ok_or(Error::ClueNotFound)?
            .clues.get(clue_index).ok_or(Error::ClueNotFound)?
            .clone();
        self.phase = GamePhase::Clue{clue};
        Ok(())
    }

    fn clue_fully_shown(&mut self) -> Result<(), Error> {
        let clue = match &self.phase {
            GamePhase::Clue { clue } => Ok(clue.clone()),
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            })
        }?;
        self.phase = GamePhase::Buzzing{clue};
        Ok(())
    }

    fn buzz(&mut self, contestant_index: usize) -> Result<(), Error> {
        match &mut self.phase {
            // allow buzzing in the buzzing phase
            GamePhase::Buzzing { clue } => {
                let clue = clue.clone();
                self.indicate_contestant(contestant_index)?;
                self.phase = GamePhase::Buzzed{clue};
                Ok(())
            },
            // allow toggling the indication lights in the lobby or while picking
            GamePhase::Connecting | GamePhase::Picking => {
                if let Some(i) = self.indications.iter().position(|c| c == &contestant_index) {
                    self.indications.remove(i);
                    self.contestants
                        .get_mut(contestant_index)
                        .ok_or(Error::ContestantNotFound)?
                        .indicate = false;
                } else {
                    self.indicate_contestant(contestant_index)?;
                }
                Ok(())
            },
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            })
        }
    }

    fn set_wage(&mut self, points: Points) -> Result<(), Error> {
        match &mut self.phase {
            GamePhase::Waging { clue } => {
                let clue = clue.clone();
                self.phase = GamePhase::Clue { clue };
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn accept_answer(&mut self) -> Result<(), Error> {
        match &mut self.phase {
            GamePhase::Buzzed { clue } => {
                let clue = clue.clone();
                self.phase = GamePhase::Resolution { clue };
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn reject_answer(&mut self) -> Result<(), Error> {
        match &mut self.phase {
            GamePhase::Buzzed { clue } => {
                let clue = clue.clone();
                self.phase = GamePhase::Buzzing { clue };
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn reveal_hint(&mut self) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Resolution { .. }) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        Ok(())
    }

    fn finish_clue(&mut self) -> Result<(), Error> {
        match &mut self.phase {
            GamePhase::Resolution { .. } => {
                if self.board.categories.iter().all(|c| c.clues.is_empty()) {
                    self.phase = GamePhase::Score;
                } else {
                    self.phase = GamePhase::Picking;
                }
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn indicate_contestant(&mut self, contestant_index: usize) -> Result<(), Error> {
        self.contestants
            .get_mut(contestant_index)
            .ok_or(Error::ContestantNotFound)?
            .indicate = true;
        if !self.indications.contains(&contestant_index) {
            self.indications.push(contestant_index);
        }
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
    WrongPhase { is: GamePhase },
    ContestantNotFound,
    NoContestants,
    ClueNotFound,
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
                    clues: (1..qs)
                        .into_iter()
                        .map(|q| Clue {
                            clue: format!("clue {}", q),
                            response: format!("clue {}", q),
                            hint: format!("clue {}", q),
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
            Event::LoadBoard(get_test_board(2, 2)),
            Event::OpenLobby,
            Event::ConnectContestant {
                name_hint: "test_contestant_hint".to_owned(),
            },
            Event::NameContestant {
                index: 0,
                name: "Test Contestant".to_owned(),
            },
            Event::StartGame,
            Event::Pick{category_index: 0, clue_index: 0},
            Event::ClueFullyShown,
            Event::Buzz{contestant_index: 0},
            Event::RejectAnswer,
            Event::FinishClue,
            Event::Pick { category_index: 0, clue_index: 1 },
            Event::ClueFullyShown,
            Event::Buzz{contestant_index: 0},
            Event::AcceptAnswer,
            Event::FinishClue,
            Event::Pick { category_index: 1, clue_index: 0 },
            Event::ClueFullyShown,
            Event::Buzz{contestant_index: 0},
            Event::AcceptAnswer,
            Event::FinishClue,
            Event::Pick { category_index: 1, clue_index: 1 },
            Event::ClueFullyShown,
            Event::Buzz{contestant_index: 0},
            Event::AcceptAnswer,
            Event::FinishClue,
        ]
        .into_iter()
        .fold(g, |mut g, e| {
            g.apply(e.clone()).expect(format!("could not apply event {:?}", e).as_str());
            g
        });
        assert_eq!(r.board, get_test_board(2, 2));
        assert_eq!(r.contestants.len(), 1);
        assert_eq!(r.contestants[0].name, Some("Test Contestant".to_owned()));
        // assert_eq!(r.contestants[0].points, 10 as Points);
        // assert!(matches!(r.phase, GamePhase::Score));
    }
}
