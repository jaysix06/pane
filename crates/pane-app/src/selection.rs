//! Which rows are selected, by index into the tab's listing, with Explorer's
//! rules for clicks, Ctrl, Shift and the arrow keys. Pure logic.

use std::collections::BTreeSet;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Selection {
    set: BTreeSet<usize>,
    /// Where a Shift range starts.
    anchor: Option<usize>,
    /// The row the keyboard is on.
    focus: Option<usize>,
}

/// A key that moves the focus.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Move {
    Up,
    Down,
    PageUp,
    PageDown,
    Home,
    End,
}

impl Selection {
    pub fn indices(&self) -> impl Iterator<Item = usize> + '_ {
        self.set.iter().copied()
    }
    pub fn len(&self) -> usize {
        self.set.len()
    }
    pub fn focus(&self) -> Option<usize> {
        self.focus
    }

    pub fn clear(&mut self) {
        *self = Selection::default();
    }

    /// A click on row `i`: alone selects it; Ctrl toggles it; Shift selects
    /// from the anchor to it (adding to the rest with Ctrl too).
    pub fn click(&mut self, i: usize, ctrl: bool, shift: bool) {
        if shift {
            let anchor = self.anchor.unwrap_or(i);
            if !ctrl {
                self.set.clear();
            }
            self.set.extend(anchor.min(i)..=anchor.max(i));
            self.anchor = Some(anchor);
        } else if ctrl {
            if !self.set.remove(&i) {
                self.set.insert(i);
            }
            self.anchor = Some(i);
        } else {
            self.set.clear();
            self.set.insert(i);
            self.anchor = Some(i);
        }
        self.focus = Some(i);
    }

    /// Selects `i` alone unless it is already selected (a right click on a
    /// selected row keeps the selection, as Explorer does).
    pub fn ensure(&mut self, i: usize) {
        if !self.set.contains(&i) {
            self.click(i, false, false);
        }
    }

    pub fn select_all(&mut self, len: usize) {
        self.set = (0..len).collect();
        if self.focus.is_none() && len > 0 {
            self.focus = Some(0);
            self.anchor = Some(0);
        }
    }

    /// An arrow, Page or Home/End key over `len` rows, `page` rows a page.
    /// Returns the new focus.
    pub fn move_focus(&mut self, by: Move, shift: bool, len: usize, page: usize) -> Option<usize> {
        if len == 0 {
            return None;
        }
        let last = len - 1;
        let page = page.max(1);
        let to = match (self.focus, by) {
            (None, Move::End) => last,
            (None, _) => 0,
            (Some(f), Move::Up) => f.saturating_sub(1),
            (Some(f), Move::Down) => (f + 1).min(last),
            (Some(f), Move::PageUp) => f.saturating_sub(page),
            (Some(f), Move::PageDown) => (f + page).min(last),
            (Some(_), Move::Home) => 0,
            (Some(_), Move::End) => last,
        };
        self.click(to, false, shift);
        Some(to)
    }

    /// Carries the selection across a re-listing, by name: rows that still
    /// exist stay selected wherever they moved.
    pub fn remap(&self, old: &[impl AsRef<str>], new: &[impl AsRef<str>]) -> Selection {
        use std::collections::HashMap;
        let at: HashMap<&str, usize> = new.iter().enumerate().map(|(i, n)| (n.as_ref(), i)).collect();
        let find = |i: usize| old.get(i).and_then(|n| at.get(n.as_ref()).copied());
        Selection {
            set: self.set.iter().filter_map(|&i| find(i)).collect(),
            anchor: self.anchor.and_then(find),
            focus: self.focus.and_then(find),
        }
    }
}

/// Type-to-jump: the first row after `from` (wrapping) whose name starts
/// with `prefix`, case-insensitively. Typing one letter again cycles.
pub fn jump(names: &[impl AsRef<str>], prefix: &str, from: Option<usize>) -> Option<usize> {
    if names.is_empty() || prefix.is_empty() {
        return None;
    }
    let prefix = prefix.to_lowercase();
    // The same letter repeated ("aaa") cycles through rows starting with it.
    let repeated = prefix.chars().all(|c| Some(c) == prefix.chars().next());
    let (needle, start) = if repeated && prefix.chars().count() > 1 {
        (prefix.chars().next().unwrap().to_string(), from.map_or(0, |f| f + 1))
    } else if prefix.chars().count() == 1 {
        (prefix.clone(), from.map_or(0, |f| f + 1))
    } else {
        (prefix.clone(), from.unwrap_or(0))
    };
    let n = names.len();
    (0..n).map(|k| (start + k) % n).find(|&i| names[i].as_ref().to_lowercase().starts_with(&needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sel(s: &Selection) -> Vec<usize> {
        s.indices().collect()
    }

    #[test]
    fn clicks_ctrl_and_shift() {
        let mut s = Selection::default();
        s.click(2, false, false);
        assert_eq!(sel(&s), [2]);
        s.click(5, false, true);
        assert_eq!(sel(&s), [2, 3, 4, 5]);
        s.click(0, false, true);
        assert_eq!(sel(&s), [0, 1, 2], "range re-drawn from the same anchor");
        s.click(7, true, false);
        assert_eq!(sel(&s), [0, 1, 2, 7]);
        s.click(1, true, false);
        assert_eq!(sel(&s), [0, 2, 7]);
        s.click(9, true, true);
        assert_eq!(sel(&s), [0, 1, 2, 3, 4, 5, 6, 7, 8, 9], "ctrl+shift adds the range from the anchor (row 1, the last ctrl-click)");
    }

    #[test]
    fn right_click_keeps_a_selection_it_lands_in() {
        let mut s = Selection::default();
        s.click(1, false, false);
        s.click(3, false, true);
        s.ensure(2);
        assert_eq!(sel(&s), [1, 2, 3]);
        s.ensure(8);
        assert_eq!(sel(&s), [8]);
    }

    #[test]
    fn arrows_move_and_extend() {
        let mut s = Selection::default();
        assert_eq!(s.move_focus(Move::Down, false, 10, 4), Some(0));
        assert_eq!(s.move_focus(Move::Down, false, 10, 4), Some(1));
        assert_eq!(s.move_focus(Move::Down, true, 10, 4), Some(2));
        assert_eq!(sel(&s), [1, 2]);
        assert_eq!(s.move_focus(Move::PageDown, false, 10, 4), Some(6));
        assert_eq!(s.move_focus(Move::End, false, 10, 4), Some(9));
        assert_eq!(s.move_focus(Move::Down, false, 10, 4), Some(9));
        assert_eq!(s.move_focus(Move::Home, true, 10, 4), Some(0));
        assert_eq!(sel(&s).len(), 10);
        assert_eq!(s.move_focus(Move::Up, false, 0, 4), None);
    }

    #[test]
    fn remap_follows_names() {
        let mut s = Selection::default();
        s.click(0, false, false);
        s.click(2, true, false);
        let old = ["a", "b", "c"];
        let new = ["c", "x", "a"];
        let r = s.remap(&old, &new);
        assert_eq!(sel(&r), [0, 2]);
        assert_eq!(r.focus(), Some(0), "focus followed c");
    }

    #[test]
    fn type_to_jump() {
        let names = ["apple", "banana", "blueberry", "cherry", "Bread"];
        assert_eq!(jump(&names, "b", None), Some(1));
        assert_eq!(jump(&names, "b", Some(1)), Some(2), "one letter moves on");
        assert_eq!(jump(&names, "bb", Some(2)), Some(4), "repeated letter cycles");
        assert_eq!(jump(&names, "bl", Some(1)), Some(2));
        assert_eq!(jump(&names, "br", Some(0)), Some(4), "case-insensitive");
        assert_eq!(jump(&names, "z", None), None);
    }
}
