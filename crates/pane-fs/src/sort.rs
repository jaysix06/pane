use std::cmp::Ordering;

use crate::Entry;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Modified,
    Type,
    Size,
}

/// Compares names the way Explorer does: case-insensitive, with runs of
/// digits compared by value ("file2" before "file10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut i, mut j) = (0, 0);
    loop {
        let (x, y) = (a[i..].chars().next(), b[j..].chars().next());
        match (x, y) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let da = digit_run(&a[i..]);
                let db = digit_run(&b[j..]);
                i += da.len();
                j += db.len();
                let ta = da.trim_start_matches('0');
                let tb = db.trim_start_matches('0');
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
                // Equal values: fewer leading zeros first.
                let ord = da.len().cmp(&db.len());
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = if x.is_ascii() && y.is_ascii() {
                    x.to_ascii_lowercase().cmp(&y.to_ascii_lowercase())
                } else {
                    x.to_lowercase().cmp(y.to_lowercase())
                };
                if ord != Ordering::Equal {
                    return ord;
                }
                i += x.len_utf8();
                j += y.len_utf8();
            }
        }
    }
}

/// The leading run of ASCII digits.
fn digit_run(s: &str) -> &str {
    let end = s.bytes().position(|c| !c.is_ascii_digit()).unwrap_or(s.len());
    &s[..end]
}

/// Sorts folders first, then by `key`, ties broken by name.
pub fn sort_entries(entries: &mut [Entry], key: SortKey, descending: bool) {
    entries.sort_by(|a, b| {
        let folders = b.is_dir.cmp(&a.is_dir);
        if folders != Ordering::Equal {
            return folders;
        }
        let ord = match key {
            SortKey::Name => Ordering::Equal,
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Type => a.extension().cmp(&b.extension()),
            SortKey::Size => a.size.cmp(&b.size),
        }
        .then_with(|| natural_cmp(&a.name, &b.name));
        if descending { ord.reverse() } else { ord }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_compare_by_value() {
        let mut names = vec!["file10", "file2", "File1", "file02", "a", "B"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["a", "B", "File1", "file2", "file02", "file10"]);
    }

    #[test]
    fn folders_come_first() {
        let e = |name: &str, is_dir: bool| Entry {
            name: name.into(),
            is_dir,
            size: 0,
            modified: 0,
            hidden: false,
            system: false,
        };
        let mut v = vec![e("b.txt", false), e("z", true), e("a.txt", false), e("c", true)];
        sort_entries(&mut v, SortKey::Name, false);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["c", "z", "a.txt", "b.txt"]);
    }
}
