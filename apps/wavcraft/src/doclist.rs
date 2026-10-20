//! The order of the open documents. The current document's state lives in the window's own
//! fields; every other document is parked here, and switching swaps the two. This type only
//! keeps the order and the index, so it can be tested without a window.

/// Where document number `i` is kept.
#[derive(Debug, PartialEq, Eq)]
pub enum Where {
    /// In the window's fields: the current document.
    Current,
    /// In the parked list, at this index.
    Parked(usize),
}

/// The parked documents and the position of the current one among all of them: the full order
/// is the parked list with the current document inserted at `current`.
#[derive(Debug)]
pub struct DocList<T> {
    parked: Vec<T>,
    current: usize,
}

impl<T> Default for DocList<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> DocList<T> {
    pub fn new() -> Self {
        DocList { parked: Vec::new(), current: 0 }
    }

    /// How many documents are open, given whether the window holds a current one.
    pub fn len(&self, has_current: bool) -> usize {
        self.parked.len() + usize::from(has_current)
    }

    /// The current document's position (0 when it is the only one, or none is open).
    pub fn current(&self) -> usize {
        self.current
    }

    pub fn locate(&self, i: usize) -> Where {
        match i.cmp(&self.current) {
            std::cmp::Ordering::Equal => Where::Current,
            std::cmp::Ordering::Less => Where::Parked(i),
            std::cmp::Ordering::Greater => Where::Parked(i - 1),
        }
    }

    pub fn parked(&self) -> &[T] {
        &self.parked
    }

    pub fn parked_mut(&mut self) -> &mut [T] {
        &mut self.parked
    }

    /// A new document is about to become current, at the end. `old` is the document that was
    /// current (`None` when nothing was open); it is parked where it was.
    pub fn add(&mut self, old: Option<T>) {
        if let Some(old) = old {
            self.parked.insert(self.current.min(self.parked.len()), old);
            self.current = self.parked.len();
        } else {
            self.current = self.parked.len();
        }
    }

    /// Make document `i` current. `old` is the current one, parked in its place. Returns the
    /// document to load into the window's fields, or gives `old` back when `i` is out of range.
    pub fn switch(&mut self, i: usize, old: T) -> Result<T, T> {
        if i > self.parked.len() {
            return Err(old);
        }
        self.parked.insert(self.current.min(self.parked.len()), old);
        self.current = i;
        Ok(self.parked.remove(i))
    }

    /// The current document was closed: the one that takes its place (the next, or the last),
    /// or `None` when it was the only one.
    pub fn close_current(&mut self) -> Option<T> {
        if self.parked.is_empty() {
            self.current = 0;
            return None;
        }
        self.current = self.current.min(self.parked.len() - 1);
        Some(self.parked.remove(self.current))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A list of `n` documents named 0..n, current = the last (as after opening them in turn),
    /// with the window's own slot returned alongside.
    fn opened(n: u32) -> (DocList<u32>, u32) {
        let mut l = DocList::new();
        let mut cur = None;
        for k in 0..n {
            l.add(cur.take());
            cur = Some(k);
        }
        (l, cur.unwrap_or(u32::MAX))
    }

    #[test]
    fn opening_adds_at_the_end_and_becomes_current() {
        let (l, cur) = opened(3);
        assert_eq!((l.len(true), l.current(), cur), (3, 2, 2));
        assert_eq!(l.parked(), &[0, 1]);
        assert_eq!(l.locate(2), Where::Current);
        assert_eq!(l.locate(0), Where::Parked(0));
    }

    #[test]
    fn switching_swaps_and_keeps_the_order() {
        let (mut l, cur) = opened(3);
        assert_eq!(l.switch(0, cur), Ok(0));
        assert_eq!(l.current(), 0);
        assert_eq!(l.parked(), &[1, 2]);
        assert_eq!(l.locate(1), Where::Parked(0));
        // The window now holds document 0; switching to 1 parks it again.
        assert_eq!(l.switch(1, 0), Ok(1));
        assert_eq!((l.current(), l.parked()), (1, &[0, 2][..]));
        let cur = 1;
        // A bad index changes nothing and gives the current document back.
        assert_eq!(l.switch(3, cur), Err(1));
        assert_eq!((l.current(), l.parked()), (1, &[0, 2][..]));
    }

    #[test]
    fn closing_picks_the_next_then_the_last() {
        let (mut l, cur) = opened(3);
        assert_eq!(l.switch(1, cur), Ok(1));
        // Close doc 1 of [0,1,2]: doc 2 takes its place.
        assert_eq!(l.close_current(), Some(2));
        assert_eq!((l.current(), l.parked()), (1, &[0][..]));
        // Close the last: the one before it.
        assert_eq!(l.close_current(), Some(0));
        assert_eq!((l.current(), l.parked().len()), (0, 0));
        assert_eq!(l.close_current(), None);
        assert_eq!(l.len(false), 0);
    }

    #[test]
    fn adding_after_a_switch_goes_to_the_end() {
        let (mut l, cur) = opened(3);
        assert_eq!(l.switch(0, cur), Ok(0));
        l.add(Some(0));
        assert_eq!((l.current(), l.parked()), (3, &[0, 1, 2][..]));
    }
}
