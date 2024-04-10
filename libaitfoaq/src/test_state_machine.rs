use proptest::prelude::*;

use crate::events::*;
use crate::state::*;
use crate::*;

prop_compose! {
    fn arb_clue()(
        c in ".*",
        a in ".*",
        h in ".*",
        p in any::<Points>(),
        w in prop::bool::weighted(0.1),
        e in prop::bool::weighted(0.1),
    ) -> Clue {
        Clue { clue: c, response: a, hint: h, points: p, can_wager: w, exclusive: e, solved: false }
    }
}
prop_compose! {
    fn arb_category(nclues: usize)
                (title in ".*", clues in prop::collection::vec(arb_clue(), prop::collection::size_range(nclues)))
                -> Category {
        Category {title, clues}
    }
}
prop_compose! {
    fn arb_board()
                (ncats in 0..12, nclues in 0..12)
                (categories in prop::collection::vec(arb_category(nclues as usize), prop::collection::size_range(ncats as usize)))
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
