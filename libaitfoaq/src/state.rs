use serde::{Deserialize, Serialize};

pub type Points = i32; // JS is limited to 32bit
pub type ContestantHandle = usize;
pub type ClueHandle = (usize, usize);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameState {
    pub contestants: Vec<Contestant>,
    pub board: Board,
    pub phase: GamePhase,
    pub options: Options,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub categories: Vec<Category>,
}

impl Board {
    pub fn get(&self, clue: ClueHandle) -> Result<&Clue, super::Error> {
        Ok(self
            .categories
            .get(clue.0)
            .ok_or(super::Error::ClueNotFound)?
            .clues
            .get(clue.1)
            .ok_or(super::Error::ClueNotFound)?)
    }

    pub fn get_mut(&mut self, clue: ClueHandle) -> Result<&mut Clue, super::Error> {
        Ok(self
            .categories
            .get_mut(clue.0)
            .ok_or(super::Error::ClueNotFound)?
            .clues
            .get_mut(clue.1)
            .ok_or(super::Error::ClueNotFound)?)
    }

    pub fn mark_solved(&mut self, clue: ClueHandle) -> Result<(), super::Error> {
        self.get_mut(clue)?.solved = true;
        Ok(())
    }
}

impl Board {
    pub fn clue_rows(&self) -> Vec<Vec<(ClueHandle, Clue)>> {
        if self.categories.len() == 0 {
            return vec![];
        }
        (0..self.categories[0].clues.len())
            .map(|r| self.categories.iter().enumerate().map(move |(i,c)| ((i,r), c.clues[r].clone())).collect()).collect()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub title: String,
    pub clues: Vec<Clue>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Clue {
    /// The prompt for players, in the form of an answer
    pub clue: String,
    /// The expected answer from players
    pub response: String,
    /// More context around the question or alternative answers that the
    /// moderator might choose to accept too. Hidden from contestants.
    pub hint: String,
    /// How much a contestant wins when solving the clue. Can change e.g. with a
    /// wager.
    pub points: Points,
    /// If players can bet some or all of their points. True for example for a
    /// Daily Double clue.
    pub can_wager: bool,
    /// If the clue is exclusive to the picker for a first attempt before it
    /// get's opened up to all contestants. True for example for a Daily Double
    /// clue.
    pub exclusive: bool,
    /// If this clue was already played.
    pub solved: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Contestant {
    /// Can be renamed by the moderator and is the only name that should be
    /// shown during the game. If None, the name_hint can be used instead.
    pub name: Option<String>,
    /// A hint for the name that could be pre-filled by contestants or
    /// automatically chosen by the game client used. Like the user-agent of
    /// the contestants browser or the controller port number.
    pub name_hint: String,
    pub points: Points,
    /// If the player should be indicated with their name on a screen or a
    /// light to let the moderator know that they successfully buzzed in.
    /// More than one player can be indicated at a time, but not during regular
    /// gameplay.
    pub indicate: bool,
    /// If the controller is still connected to the game
    pub connected: bool,
}

/// The phase a [Game](crate::Game) is in. Transitians between states are
/// documented on [Event](crate::events::Event). Use
/// [Game::apply](crate::Game::apply) to transition.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum GamePhase {
    /// Loading questions and configuration. The main screen is likely not
    /// visible yet, and contestants might not be present yet. Regardless, the
    /// screen should not show any information of the board.
    Preparing,
    /// Players connecting, introducing themselves, and testing their
    /// controllers. The board is still hidden.
    Connecting,
    /// Contestants picking a question from the board
    Picking { contestant: ContestantHandle },
    /// Betting points before seeing the clue
    Waging {
        clue: ClueHandle,
        contestant: ContestantHandle,
    },
    /// The clue/prompt is shown or played to the contestants
    Clue {
        clue: ClueHandle,
        exclusive: Option<ContestantHandle>,
    },
    /// The clue is still visible, but contestants can buzz in now. Can be
    /// skipped e.g. for daily double questions.
    Buzzing { clue: ClueHandle },
    /// The indicated contestant ([Contestant::indicate]) buzzed in and
    /// can attempt to answer the clue
    Buzzed {
        clue: ClueHandle,
        contestant: ContestantHandle,
    },
    /// A correct answer was provided or all contestants failed
    Resolution {
        clue: ClueHandle,
        contestant: ContestantHandle,
    },
    /// After all clues are played the final score is shown. Either just
    /// all players with their points, or a representation of the board showing
    /// which contestant answerd the question correctly, or something
    /// completely different.
    // todo: play final jeopardy
    Score,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Options {
    // pub multiple_attempts: bool, allow contestants to buzz in again after providing a wrong answer
    // pub wrong_answer_penalty: bool, deduct points on wrong anwsers
    // pub wait_for_clue: bool, wait for the clue to be finished reading/playing once before opening up for buzzing
}

impl Default for Options {
    fn default() -> Self {
        Options {}
    }
}
