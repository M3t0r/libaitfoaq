use serde::{Deserialize, Serialize};

use crate::state::{Board, ClueHandle, ContestantHandle, Points};
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
///        optional_waging --> Clue: if can_wager == false
///        optional_waging --> Waging: if can_wager == true
///        Waging --> Clue: SetWage
///        Clue --> Buzzing: ClueFullyShown
///        Clue --> next_or_end: FinishClue
///        Buzzing --> Buzzed: Buzz
///        Buzzing --> next_or_end: FinishClue
///        Buzzed --> Clue: RejectAnswer
///        Buzzed --> Resolution: AcceptAnswer
///        Buzzed --> Resolution: FinishClue
///        Resolution --> next_or_end: FinishClue
///        next_or_end --> Score: if clues_left <= 0
///        next_or_end --> Picking: if clues_left > 0
///    }
///    Score --> [*]
/// ```
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all_fields = "snake_case", tag = "type")]
pub enum Event {
    /// Change settings before starting the game.
    /// Only allowed in [GamePhase::Preparing]. Can be repeated.
    Settings,
    /// Load a [Board] of clues.
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
    DisconnectContestant { contestant: ContestantHandle },
    /// Reconnect a [Contestant] and resume the game.
    ReconnectContestant { contestant: ContestantHandle },
    /// Properly name a [Contestant]. This might happen during an introduction
    /// round. Can also happen after [GamePhase::Connecting].
    NameContestant { index: usize, name: String },
    /// Transition from [GamePhase::Connecting] to [GamePhase::Picking]. No new
    /// [Contestants](Contestant) can connect afterwards.
    StartGame,
    /// Transition from [GamePhase::Picking] to [GamePhase::Waging] or
    /// [GamePhase::Clue] depending on
    /// [Clue::can_wager](crate::state::Clue::can_wager) of the picked clue.
    Pick { clue: ClueHandle },

    /// Transition from [GamePhase::Waging] to [GamePhase::Clue].
    /// A [Contestant] waging some of their [Points].
    SetWage { points: Points },

    /// Transition from [GamePhase::Clue] to [GamePhase::Buzzing]. During
    /// [GamePhase::Clue] [Contestants](Contestant) can't buzz in so everyone
    /// gets a chance to fully hear the prompt.
    // todo: make skippable with setting so contestants can buzz in immedieately
    ClueFullyShown,

    /// A [Contestant] buzzing in. Transtion from [GamePhase::Buzzing] to
    /// [GamePhase::Buzzed]
    Buzz { contestant: ContestantHandle },

    /// Transition from [GamePhase::Buzzed] to [GamePhase::Resolution].
    AcceptAnswer,
    /// Transition from [GamePhase::Buzzed] to [GamePhase::Buzzing].
    RejectAnswer,

    /// Reveal the moderator hint to the contestants in [GamePhase::Resolution]
    RevealHint,
    /// Transition from [GamePhase::Resolution] to [GamePhase::Score] or back to
    /// [GamePhase::Picking]. Can also be used to skip answering a prompt from
    /// [GamePhase::Clue], [GamePhase::Buzzing], or [GamePhase::Buzzed] without
    /// awarding/changing points.
    FinishClue,
}
