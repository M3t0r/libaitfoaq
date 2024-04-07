use serde::{Deserialize, Serialize};

use crate::state::Board;
#[cfg(doc)]
use crate::state::{Contestant, GamePhase, GameState};

#[cfg_attr(doc, aquamarine::aquamarine)]
/// ```mermaid
/// stateDiagram-v2
///    state GameLoop: Game Loop
///    state optional_waging <<choice>>
///    state next_or_end <<choice>>
///
///    [*] --> Preparing
///    Preparing --> Connecting: OpenLobby
///    Connecting --> Picking: StartGame
///    state GameLoop {
///        Picking --> optional_waging: Pick
///        optional_waging --> Prompt: if can_wager == false
///        optional_waging --> Waging: if can_wager == true
///        Waging --> Prompt: SetWage
///        Prompt --> Buzzing: PromptFullyShown
///        Buzzing --> Buzzed: Buzz
///        Buzzed --> Prompt: RejectAnswer
///        Buzzed --> Resolution: AcceptAnswer
///        Resolution --> next_or_end: FinishQuestion
///        next_or_end --> Score: if questions_left <= 0
///        next_or_end --> Picking: if questions_left > 0
///    }
///    Score --> [*]
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all_fields = "snake_case", tag = "type")]
pub enum Event {
    /// Change settings before starting the game.
    /// Only allowed in [GamePhase::Preparing]. Can be repeated.
    Settings,
    /// Load a [Board] of questions.
    /// Only allowed in [GamePhase::Preparing]. Can be repeated, which replaces
    /// the already loaded board.
    LoadBoard(Board),
    /// Allow players to connect.
    /// Transitions from [GamePhase::Preparing] to [GamePhase::Connecting].
    OpenLobby,

    /// Initial registration of a contestant. Adds a [Contestant] to [GameState].
    /// Only allowed in [GamePhase::Connecting].
    ConnectContestant { name_hint: String },
    /// Mark a [Contestant] as disconnected. This does not remove them, they can
    /// join at a later time, and optionally halt the game until then.
    DisconnectContestant,
    /// Reconnect a [Contestant] and resume the game.
    ReconnectContestant,
    /// Properly name a [Contestant]. This might happen during an introduction
    /// round. Can also happen after [GamePhase::Connecting].
    NameContestant { index: usize, name: String },
    /// Transition from [GamePhase::Connecting] to [GamePhase::Picking]. No new
    /// [Contestants](Contestant) can connect afterwards.
    StartGame,
    /// Transition from [GamePhase::Picking] to [GamePhase::Waging] or
    /// [GamePhase::Prompt] depending on
    /// [Question::can_wager](crate::state::Question::can_wager) of the picked
    /// question.
    Pick,

    /// Transition from [GamePhase::Waging] to [GamePhase::Prompt].
    /// A [Contestant] waging some of their [Points](crate::state::Points).
    SetWage,

    /// Transition from [GamePhase::Prompt] to [GamePhase::Buzzing]. During
    /// [GamePhase::Prompt] [Contestants](Contestant) can't buzz in so everyone
    /// gets a chance to fully hear the prompt.
    // todo: make skippable with setting so contestants can buzz in immedieately
    PromptFullyShown,

    /// A [Contestant] buzzing in. Transtion from [GamePhase::Buzzing] to
    /// [GamePhase::Buzzed]
    Buzz,

    /// Transition from [GamePhase::Buzzed] to [GamePhase::Resolution].
    AcceptAnswer,
    /// Transition from [GamePhase::Buzzed] to [GamePhase::Buzzing].
    RejectAnswer,

    /// Reveal the moderator hint to the contestants in [GamePhase::Resolution]
    RevealHint,
    /// Transition from [GamePhase::Resolution] to [GamePhase::Score] or back to
    /// [GamePhase::Picking].
    FinishQuestion,
}
