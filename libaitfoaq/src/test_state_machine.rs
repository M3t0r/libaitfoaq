use proptest::prelude::*;

use crate::*;
use crate::events::*;
use crate::state::*;

prop_compose! {
    fn arb_question()(
        q in ".*",
        a in ".*",
        h in ".*",
        p in any::<Points>(),
        w in prop::bool::weighted(0.1),
        e in prop::bool::weighted(0.1),
    ) -> Question {
        Question { question: q, answer: a, hint: h, points: p, can_wager: w, exclusive: e }
    }
}
prop_compose! {
    fn arb_category(qs: usize)
                (title in ".*", questions in prop::collection::vec(arb_question(), prop::collection::size_range(qs)))
                -> Category {
        Category {title, questions}
    }
}
prop_compose! {
    fn arb_board()
                (cs in 0..12, qs in 0..12)
                (categories in prop::collection::vec(arb_category(qs as usize), prop::collection::size_range(cs as usize)))
                -> Board {
        Board {categories}
    }
}

proptest! {
    #[test]
    fn load_arbitrary_boards(board in arb_board()) {
        let mut g = Game::default();
        let r = g.apply(Event::LoadBoard(board.clone())).expect("board didn't load");
        assert_eq!(r.board, board);
    }
}
