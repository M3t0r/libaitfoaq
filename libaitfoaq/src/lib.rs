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
    indications: Vec<ContestantHandle>,
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
            Event::Pick { clue } => self.pick(clue)?,
            Event::ClueFullyShown => self.clue_fully_shown()?,
            Event::Buzz { contestant } => self.buzz(contestant)?,
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

    fn pick(&mut self, clue_handle: ClueHandle) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Picking) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        let (category_index, clue_index) = clue_handle;
        let clue = self
            .board
            .categories
            .get(category_index)
            .ok_or(Error::ClueNotFound)?
            .clues
            .get(clue_index)
            .ok_or(Error::ClueNotFound)?
            .clone();
        // let exclusive = if clue.exclusive { todo!() } else { None };
        // self.phase = GamePhase::Clue{clue, exclusive};
        self.phase = GamePhase::Clue { clue: clue_handle };
        Ok(())
    }

    fn clue_fully_shown(&mut self) -> Result<(), Error> {
        let clue = match self.phase {
            GamePhase::Clue { clue } => Ok(clue),
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }?;
        self.phase = GamePhase::Buzzing { clue };
        Ok(())
    }

    fn buzz(&mut self, contestant_index: usize) -> Result<(), Error> {
        match self.phase {
            // allow buzzing in the buzzing phase
            GamePhase::Buzzing { clue } => {
                self.indicate_contestant(contestant_index)?;
                self.phase = GamePhase::Buzzed { clue };
                Ok(())
            }
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
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn set_wage(&mut self, points: Points) -> Result<(), Error> {
        match self.phase {
            GamePhase::Waging { clue } => {
                self.phase = GamePhase::Clue { clue };
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn accept_answer(&mut self) -> Result<(), Error> {
        match self.phase {
            GamePhase::Buzzed { clue } => {
                self.phase = GamePhase::Resolution { clue };
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn reject_answer(&mut self) -> Result<(), Error> {
        match self.phase {
            GamePhase::Buzzed { clue } => {
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
        let (phase, clue) = match self.phase {
            GamePhase::Clue { clue } => (None, clue),
            GamePhase::Buzzing { clue } => (None, clue),
            GamePhase::Buzzed { clue } => (Some(GamePhase::Resolution { clue }), clue),
            GamePhase::Resolution { clue } => (None, clue),
            _ => {
                return Err(Error::WrongPhase {
                    is: self.phase.clone(),
                })
            }
        };
        self.board.mark_solved(clue)?;
        self.phase = phase.unwrap_or(self.next_or_end());
        Ok(())
    }

    fn next_or_end(&mut self) -> GamePhase {
        if self
            .board
            .categories
            .iter()
            .flat_map(|c| c.clues.iter())
            .all(|c| c.solved)
        {
            GamePhase::Score
        } else {
            GamePhase::Picking
        }
    }

    fn indicate_contestant(&mut self, contestant: ContestantHandle) -> Result<(), Error> {
        self.contestants
            .get_mut(contestant)
            .ok_or(Error::ContestantNotFound)?
            .indicate = true;
        if !self.indications.contains(&contestant) {
            self.indications.push(contestant);
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
            categories: (1..(cs + 1))
                .into_iter()
                .map(|c| Category {
                    title: format!("Category {}", c),
                    clues: (1..(qs + 1))
                        .into_iter()
                        .map(|q| Clue {
                            clue: format!("clue {}", q),
                            response: format!("clue {}", q),
                            hint: format!("clue {}", q),
                            points: 100 * q as Points,
                            can_wager: q == 4 && c == 2,
                            exclusive: q == 4 && c == 2,
                            solved: false,
                        })
                        .collect(),
                })
                .collect(),
        }
    }

    #[test]
    fn it_works() {
        let g = Game::default();
        let mut test_board = get_test_board(2, 2);
        let r = vec![
            Event::LoadBoard(test_board.clone()),
            Event::OpenLobby,
            Event::ConnectContestant {
                name_hint: "test_contestant_hint".to_owned(),
            },
            Event::NameContestant {
                index: 0,
                name: "Test Contestant".to_owned(),
            },
            Event::StartGame,
            Event::Pick { clue: (0, 0) },
            Event::ClueFullyShown,
            Event::Buzz { contestant: 0 },
            Event::RejectAnswer,
            Event::FinishClue,
            Event::Pick { clue: (0, 1) },
            Event::ClueFullyShown,
            Event::Buzz { contestant: 0 },
            Event::AcceptAnswer,
            Event::FinishClue,
            Event::Pick { clue: (1, 0) },
            Event::ClueFullyShown,
            Event::Buzz { contestant: 0 },
            Event::AcceptAnswer,
            Event::FinishClue,
            Event::Pick { clue: (1, 1) },
            Event::ClueFullyShown,
            Event::Buzz { contestant: 0 },
            Event::AcceptAnswer,
            Event::FinishClue,
        ]
        .into_iter()
        .fold(g, |mut g, e| {
            g.apply(e.clone())
                .expect(format!("could not apply event {:?}", e).as_str());
            g
        });

        // mark all clues as solved in our comparison
        for clue in test_board
            .categories
            .iter_mut()
            .flat_map(|c| c.clues.iter_mut())
        {
            clue.solved = true;
        }

        assert_eq!(r.board, test_board);
        assert_eq!(r.contestants.len(), 1);
        assert_eq!(r.contestants[0].name, Some("Test Contestant".to_owned()));
        // assert_eq!(r.contestants[0].points, 10 as Points);
        assert!(matches!(r.phase, GamePhase::Score));
    }
}
