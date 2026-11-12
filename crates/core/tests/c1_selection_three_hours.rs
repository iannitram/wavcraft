//! C1.1 (issue #1170): every selection gesture is exact to the sample at the far end of a
//! three-hour 48 kHz file (518,400,000 frames).

use wavcraft_core::view::{Edge, GRAB_PX, Selection, Viewport};

const TOTAL: usize = 518_400_000;

/// A viewport 1,000 frames before the file's end, at 0.5 frames per pixel.
fn view() -> Viewport {
    Viewport { start: 518_399_000.0, frames_per_px: 0.5 }
}

#[test]
fn gestures_are_exact_at_three_hours() {
    // (1) The pixel under the pointer maps to the exact frame.
    assert_eq!(view().frame_at(1998.0), 518_399_999.0);

    // (2) A plain drag from pixel 200 to pixel 1998 selects 518_399_100..518_399_999.
    {
        let v = view();
        let sel = Selection { anchor: v.frame_at(200.0) as usize, head: v.frame_at(1998.0) as usize };
        assert_eq!((sel.start(), sel.end()), (518_399_100, 518_399_999));
    }

    // (3) What is on screen is clamped to the file; select-all covers the whole file.
    {
        let sel = Selection::visible(&view(), 4000.0, TOTAL);
        assert_eq!((sel.start(), sel.end()), (518_399_000, 518_400_000));
    }
    {
        let sel = Selection::all(TOTAL);
        assert_eq!((sel.start(), sel.end()), (0, 518_400_000));
    }

    // (4) Pixel 1003 is 3 px from the end edge (518_399_500), inside GRAB_PX.
    // Checked before any extend.
    {
        let sel = Selection { anchor: 518_399_100, head: 518_399_500 };
        assert_eq!(sel.edge_near(&view(), 1003.0, GRAB_PX), Some(Edge::End));
    }

    // (5) Edge drag: grab the end, move it to 518_399_999.
    {
        let mut sel = Selection { anchor: 518_399_100, head: 518_399_500 };
        sel.grab(Edge::End);
        sel.head = 518_399_999;
        assert_eq!((sel.start(), sel.end()), (518_399_100, 518_399_999));
    }

    // (6) Shift-click at 518_399_999 moves the nearer edge (the end) there.
    {
        let mut sel = Selection { anchor: 518_399_100, head: 518_399_500 };
        sel.extend_to(518_399_999);
        assert_eq!((sel.start(), sel.end()), (518_399_100, 518_399_999));
    }
}
