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

/// How to sort a folder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortOptions {
    pub key: SortKey,
    pub descending: bool,
    pub folders_first: bool,
    /// Digits compared by value ("file2" before "file10"), as Explorer does.
    pub natural: bool,
}

impl Default for SortOptions {
    fn default() -> Self {
        SortOptions { key: SortKey::Name, descending: false, folders_first: true, natural: true }
    }
}

/// Sorts by `opts.key`, ties broken by name; folders stay first whichever
/// way it runs when `folders_first` is set.
pub fn sort_entries(entries: &mut [Entry], opts: SortOptions) {
    sort_entries_by_size(entries, opts, |e| e.size);
}

/// As [`sort_entries`], with each entry's size for a Size sort given by
/// `size`: folders sized by what's inside them, which a listing doesn't know.
pub fn sort_entries_by_size(entries: &mut [Entry], opts: SortOptions, size: impl Fn(&Entry) -> u64) {
    let names = |a: &Entry, b: &Entry| {
        if opts.natural {
            natural_cmp(&a.name, &b.name)
        } else {
            a.name.to_lowercase().cmp(&b.name.to_lowercase())
        }
    };
    entries.sort_by(|a, b| {
        if opts.folders_first {
            let folders = b.is_dir.cmp(&a.is_dir);
            if folders != Ordering::Equal {
                return folders;
            }
        }
        let ord = match opts.key {
            SortKey::Name => Ordering::Equal,
            SortKey::Modified => a.modified.cmp(&b.modified),
            SortKey::Type => a.extension().cmp(&b.extension()),
            SortKey::Size => size(a).cmp(&size(b)),
        }
        .then_with(|| names(a, b));
        if opts.descending { ord.reverse() } else { ord }
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
    fn size_sort_takes_folder_sizes_given() {
        let e = |name: &str, is_dir: bool, size: u64| Entry {
            name: name.into(),
            is_dir,
            size,
            modified: 0,
            hidden: false,
            system: false,
        };
        let mut v = vec![e("small", true, 0), e("big", true, 0), e("f", false, 5)];
        let opts = SortOptions { key: SortKey::Size, descending: true, folders_first: true, natural: true };
        sort_entries_by_size(&mut v, opts, |x| match x.name.as_str() {
            "big" => 900,
            "small" => 10,
            _ => x.size,
        });
        let names: Vec<&str> = v.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(names, ["big", "small", "f"]);
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
        sort_entries(&mut v, SortOptions::default());
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["c", "z", "a.txt", "b.txt"]);

        sort_entries(&mut v, SortOptions { descending: true, ..Default::default() });
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["z", "c", "b.txt", "a.txt"], "folders still first");

        sort_entries(&mut v, SortOptions { folders_first: false, ..Default::default() });
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["a.txt", "b.txt", "c", "z"]);
    }
}
