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
            options: Options::default(),
        }
    }

    pub fn apply(&mut self, event: Event) -> Result<GameState, Error> {
        match event {
            Event::LoadBoard(board) => self.load_board(board)?,
            Event::OpenLobby => self.open_lobby()?,
            Event::ConnectContestant { name_hint } => self.connect_contestant(name_hint)?,
            Event::ReconnectContestant { contestant } => self.reconnect_contestant(contestant)?,
            Event::DisconnectContestant { contestant } => self.disconnect_contestant(contestant)?,
            Event::NameContestant { index, name } => self.name_contestant(index, name)?,
            Event::AwardPoints { contestant, points } => self.modify_score(contestant, points as i32)?,
            Event::RevokePoints { contestant, points } => self.modify_score(contestant, -1*(points as i32))?,
            Event::StartGame => self.start_game()?,
            Event::Pick { clue } => self.pick(clue)?,
            Event::ClueFullyShown => self.clue_fully_shown()?,
            Event::Buzz { contestant } => self.buzz(contestant)?,
            Event::SetWage { points } => self.set_wage(points)?,
            Event::AcceptAnswer => self.accept_answer()?,
            Event::RejectAnswer => self.reject_answer()?,
            Event::RevealHint => self.reveal_hint()?,
            Event::FinishClue => self.finish_clue()?,
            _ => todo!("other events"),
        }
        Ok(self.get_game_state())
    }

    pub fn get_game_state(&self) -> GameState {
        GameState {
            contestants: self.contestants.clone(),
            board: self.board.clone(),
            phase: self.phase.clone(),
            options: self.options.clone(),
        }
    }

    /// When loading game state from a file, no contestants are actually connected
    pub fn mark_all_contestants_as_disconnected(&mut self) {
        for c in self.contestants.iter_mut() {
            c.connected = false;
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

    fn reconnect_contestant(&mut self, index: ContestantHandle) -> Result<(), Error> {
        self.contestants
            .get_mut(index)
            .ok_or(Error::ContestantNotFound)?
            .connected = true;
        Ok(())
    }

    fn disconnect_contestant(&mut self, index: ContestantHandle) -> Result<(), Error> {
        self.contestants
            .get_mut(index)
            .ok_or(Error::ContestantNotFound)?
            .connected = false;
        Ok(())
    }

    fn name_contestant(&mut self, index: ContestantHandle, name: String) -> Result<(), Error> {
        self.contestants
            .get_mut(index)
            .ok_or(Error::ContestantNotFound)?
            .name = Some(name);
        Ok(())
    }

    fn modify_score(&mut self, index: ContestantHandle, points: Points) -> Result<(), Error> {
        self.contestants
            .get_mut(index)
            .ok_or(Error::ContestantNotFound)?
            .points += points;
        Ok(())
    }

    fn start_game(&mut self) -> Result<(), Error> {
        if !matches!(&self.phase, GamePhase::Connecting) {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        }
        if self.contestants.is_empty() {
            return Err(Error::NoContestants);
        }
        self.phase = GamePhase::Picking {
            contestant: self.random_contestant(),
        };
        for (i, contestant) in self.contestants.iter_mut().enumerate() {
            contestant.indicate = false;
        }
        Ok(())
    }

    fn pick(&mut self, clue: ClueHandle) -> Result<(), Error> {
        let GamePhase::Picking { contestant } = self.phase else {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        };
        self.phase = GamePhase::Clue {
            clue,
            exclusive: self.board.get(&clue)?.exclusive.then_some(contestant),
        };
        Ok(())
    }

    fn clue_fully_shown(&mut self) -> Result<(), Error> {
        self.phase = match self.phase {
            GamePhase::Clue {
                clue,
                exclusive: None,
            } => GamePhase::Buzzing { clue },
            GamePhase::Clue {
                clue,
                exclusive: Some(contestant),
            } => GamePhase::Buzzed { clue, contestant },
            _ => {
                return Err(Error::WrongPhase {
                    is: self.phase.clone(),
                })
            }
        };
        Ok(())
    }

    fn buzz(&mut self, contestant_index: usize) -> Result<(), Error> {
        match self.phase {
            // allow buzzing in the buzzing phase
            GamePhase::Buzzing { clue } => {
                self.indicate_contestant(contestant_index)?;
                self.phase = GamePhase::Buzzed {
                    clue,
                    contestant: contestant_index,
                };
                Ok(())
            }
            // allow toggling the indication lights in the lobby and the end
            GamePhase::Connecting | GamePhase::Score => {
                self.contestants
                    .get_mut(contestant_index)
                    .ok_or(Error::ContestantNotFound)?
                    .indicate ^= true;
                Ok(())
            }
            _ => Err(Error::WrongPhase {
                is: self.phase.clone(),
            }),
        }
    }

    fn set_wage(&mut self, points: Points) -> Result<(), Error> {
        let GamePhase::Waging { clue, contestant } = self.phase else {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        };
        // todo: modify points of clue to wager
        self.phase = GamePhase::Clue {
            clue,
            exclusive: Some(contestant),
        };
        Ok(())
    }

    fn accept_answer(&mut self) -> Result<(), Error> {
        let GamePhase::Buzzed { clue, contestant } = self.phase else {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        };
        let points = self.board.get(&clue)?.points;
        let c = self.contestants.get_mut(contestant).ok_or(Error::ContestantNotFound)?;
        c.points += points;
        c.indicate = false;
        self.phase = GamePhase::Resolution { clue, contestant, show_hint: false };
        Ok(())
    }

    fn reject_answer(&mut self) -> Result<(), Error> {
        let GamePhase::Buzzed { clue, contestant } = self.phase else {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        };
        let points = self.board.get(&clue)?.points;
        let c = self.contestants.get_mut(contestant).ok_or(Error::ContestantNotFound)?;
        c.points -= points;
        c.indicate = false;
        self.phase = GamePhase::Buzzing { clue };
        Ok(())
    }

    fn reveal_hint(&mut self) -> Result<(), Error> {
        let GamePhase::Resolution { clue, contestant, .. } = self.phase else {
            return Err(Error::WrongPhase {
                is: self.phase.clone(),
            });
        };
        self.phase = GamePhase::Resolution { clue, contestant, show_hint: true };
        Ok(())
    }

    fn finish_clue(&mut self) -> Result<(), Error> {
        match self.phase {
            GamePhase::Clue { clue, exclusive } => {
                self.board.mark_solved(&clue)?;
                self.phase = GamePhase::Resolution {
                    clue,
                    contestant: exclusive.unwrap_or_else(|| self.random_contestant()),
                    show_hint: false
                };
            }
            GamePhase::Buzzing { clue } => {
                self.board.mark_solved(&clue)?;
                self.phase = GamePhase::Resolution {
                    clue,
                    contestant: self.random_contestant(),
                    show_hint: false
                };
            }
            GamePhase::Buzzed { clue, contestant } => {
                self.board.mark_solved(&clue)?;
                self.phase = GamePhase::Resolution { clue, contestant, show_hint: false };
            }
            GamePhase::Resolution { clue, contestant, .. } => {
                self.board.mark_solved(&clue)?;
                self.phase = self.next_or_end(Some(contestant));
            }
            _ => {
                return Err(Error::WrongPhase {
                    is: self.phase.clone(),
                })
            }
        };
        for c in self.contestants.iter_mut() {
            c.indicate = false;
        }
        Ok(())
    }

    fn next_or_end(&mut self, contestant: Option<ContestantHandle>) -> GamePhase {
        if self
            .board
            .categories
            .iter()
            .flat_map(|c| c.clues.iter())
            .all(|c| c.solved)
        {
            GamePhase::Score
        } else {
            GamePhase::Picking {
                contestant: contestant.unwrap_or_else(|| self.random_contestant()),
            }
        }
    }

    fn indicate_contestant(&mut self, contestant_handle: ContestantHandle) -> Result<(), Error> {
        for (i, contestant) in self.contestants.iter_mut().enumerate() {
            contestant.indicate = i == contestant_handle;
        }
        Ok(())
    }

    /// Draws a random contestant based on the memory layout of all contestants names, which varies
    /// from match to match, and their points, which varies from round to round.
    ///
    /// Two consecutive calls will return the same contestant when neither the points nor their
    /// names have changed. One could argue that's a feature. I do.
    ///
    /// Initialisator: https://xkcd.com/221/
    fn random_contestant(&self) -> ContestantHandle {
        let random = self.contestants.iter().fold(
            4, // fair dice roll
            |entropy, c| {
                entropy
                    ^ (c.name.as_ref().unwrap_or(&c.name_hint).as_str().as_ptr() as usize)
                        .rotate_left(c.points as u32)
            },
        );

        dbg!(random, random % self.contestants.len());
        random % self.contestants.len()
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
        assert_eq!(r.contestants[0].points, 500 as Points);
        assert!(matches!(r.phase, GamePhase::Score));
    }
}
