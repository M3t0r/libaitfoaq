use serde::{Deserialize, Serialize};

pub type Points = i32; // JS is limited to 32bit

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GameState {
    pub contestants: Vec<Contestant>,
    /// The indexes of indicated contestants in [contestants](GameState::contestants). Ordered by the
    /// time they buzzed in, oldest first.
    pub indicated_contestants: Vec<usize>,
    pub board: Board,
    pub phase: GamePhase,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Board {
    pub categories: Vec<Category>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Category {
    pub title: String,
    pub questions: Vec<Question>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Question {
    /// The prompt for players, in the form of an answer
    pub question: String,
    /// The expected answer from players
    pub answer: String,
    /// More context around the question or alternative answers that the
    /// moderator might choose to accept too. Hidden from contestants.
    pub hint: String,
    /// How much a contestant wins when solving the prompt. Can change e.g.
    /// with a wager.
    pub points: Points,
    /// If players can bet some or all of their points. True for example for a
    /// Daily Double question.
    pub can_wager: bool,
    /// If the question is exclusive to the picker for a first attempt before
    /// it get's opened up to all contestants. True for example for a Daily
    /// Double question.
    pub exclusive: bool,
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
    Picking,
    /// Betting points before seeing the prompt
    Waging,
    /// The prompt/clue/question is shown or played to the contestants
    Prompt,
    /// The prompt is still visible, but contestants can buzz in now. Can be
    /// skipped e.g. for daily double questions.
    Buzzing,
    /// The indicated contestant ([Contestant::indicate]) buzzed in and
    /// can attempt to answer the prompt
    Buzzed,
    /// A correct answer was provided or all contestants failed
    Resolution,
    /// After all questions are played the final score is shown. Either just
    /// all players with their points, or a representation of the board showing
    /// which contestant answerd the question correctly, or something
    /// completely different.
    Score,
}
