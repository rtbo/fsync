#![allow(clippy::non_canonical_partial_ord_impl)]
//! A path module to represent paths in a repo.
//! In fsync, fsync::path is used for repository paths, where as
//! camino is used for file system.
//! There is on purpose no automatic conversion provided between this module and camino.
use std::{borrow, cmp, fmt, hash, iter::FusedIterator, ops, str};

pub use camino::{Utf8Path as FsPath, Utf8PathBuf as FsPathBuf};
use serde::{Deserialize, Serialize};
use typescript_type_def::{type_expr, TypeDef};

/// Error of normalization
#[derive(Clone, Debug)]
pub struct NormalizeError(pub PathBuf);

impl fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Path can't be normalized: `{}`", self.0)
    }
}

impl std::error::Error for NormalizeError {}

#[must_use]
pub fn is_separator(c: char) -> bool {
    c.is_ascii() && is_sep_byte(c as u8)
}

pub const SEPARATOR: char = '/';
pub const SEPARATOR_STR: &str = "/";

#[inline]
fn is_sep_byte(b: u8) -> bool {
    b == b'/'
}

#[inline]
fn has_root(path: &[u8]) -> bool {
    !path.is_empty() && path[0] == b'/'
}

// basic workhorse for splitting stem and extension
fn rsplit_file_at_dot(file: &str) -> (Option<&str>, Option<&str>) {
    if file == ".." {
        return (Some(file), None);
    }

    let mut iter = file.rsplitn(2, '.');
    let after = iter.next();
    let before = iter.next();
    if before == Some("") {
        (Some(file), None)
    } else {
        (before, after)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Component<'a> {
    RootDir,
    CurDir,
    ParentDir,
    Normal(&'a str),
}

impl<'a> Component<'a> {
    #[must_use = "`self` will be dropped if the result is not used"]
    pub fn as_str(self) -> &'a str {
        match self {
            Component::RootDir => "/",
            Component::CurDir => ".",
            Component::ParentDir => "..",
            Component::Normal(comp) => comp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum State {
    StartDir = 0,
    Body = 1,
    Done = 2,
}

/// Iterator for components
#[derive(Clone)]
pub struct Components<'a> {
    path: &'a [u8],
    has_root: bool,
    front: State,
    back: State,
}

impl<'a> Components<'a> {
    // Given the iteration so far, how much of the pre-State::Body path is left?
    #[inline]
    fn len_before_body(&self) -> usize {
        let root = if self.front <= State::StartDir && self.has_root {
            1
        } else {
            0
        };
        let cur_dir = if self.front <= State::StartDir && self.include_cur_dir() {
            1
        } else {
            0
        };
        root + cur_dir
    }

    #[inline]
    fn finished(&self) -> bool {
        self.front == State::Done || self.back == State::Done || self.front > self.back
    }

    #[must_use]
    pub fn as_path(&self) -> &'a Path {
        let mut comps = self.clone();
        if comps.front == State::Body {
            comps.trim_left();
        }
        if comps.back == State::Body {
            comps.trim_right();
        }
        unsafe { Path::from_utf8_unchecked(comps.path) }
    }

    fn has_root(&self) -> bool {
        self.has_root
    }

    // Should the normalized path include a leading . ?
    fn include_cur_dir(&self) -> bool {
        if self.has_root() {
            return false;
        }
        let mut iter = self.path.iter();
        match (iter.next(), iter.next()) {
            (Some(&b'.'), None) => true,
            (Some(&b'.'), Some(&b)) => is_sep_byte(b),
            _ => false,
        }
    }

    // parse a given byte sequence following the str encoding into
    // the corresponding path component
    unsafe fn parse_single_component<'b>(&self, comp: &'b [u8]) -> Option<Component<'b>> {
        match comp {
            b"." => None, //normalized away except at the beginning (handled by include_cur_dir)
            b".." => Some(Component::ParentDir),
            b"" => None,
            _ => Some(Component::Normal(unsafe { str::from_utf8_unchecked(comp) })),
        }
    }

    // parse a component from the left, saying how many bytes to consume to
    // remove the component
    fn parse_next_component(&self) -> (usize, Option<Component<'a>>) {
        debug_assert!(self.front == State::Body);
        let (extra, comp) = match self.path.iter().position(|b| is_sep_byte(*b)) {
            None => (0, self.path),
            Some(i) => (1, &self.path[..i]),
        };
        // SAFETY: `comp` is a valid substring, since it is split on a separator.
        (comp.len() + extra, unsafe {
            self.parse_single_component(comp)
        })
    }

    // parse a component from the right, saying how many bytes to consume to
    // remove the component
    fn parse_next_component_back(&self) -> (usize, Option<Component<'a>>) {
        debug_assert!(self.back == State::Body);
        let start = self.len_before_body();
        let (extra, comp) = match self.path[start..].iter().rposition(|b| is_sep_byte(*b)) {
            None => (0, &self.path[start..]),
            Some(i) => (1, &self.path[start + i + 1..]),
        };
        // SAFETY: `comp` is a valid substring, since it is split on a separator.
        (comp.len() + extra, unsafe {
            self.parse_single_component(comp)
        })
    }

    // trim away repeated separators (i.e., empty components) on the left
    fn trim_left(&mut self) {
        while !self.path.is_empty() {
            let (size, comp) = self.parse_next_component();
            if comp.is_some() {
                return;
            } else {
                self.path = &self.path[size..];
            }
        }
    }

    // trim away repeated separators (i.e., empty components) on the right
    fn trim_right(&mut self) {
        while self.path.len() > self.len_before_body() {
            let (size, comp) = self.parse_next_component_back();
            if comp.is_some() {
                return;
            } else {
                self.path = &self.path[..self.path.len() - size];
            }
        }
    }
}

impl<'a> PartialEq for Components<'a> {
    #[inline]
    fn eq(&self, other: &Components<'a>) -> bool {
        let Components {
            path: _,
            front: _,
            back: _,
            has_root: _,
        } = self;

        // Fast path for exact matches, e.g. for hashmap lookups.
        if self.path.len() == other.path.len()
            && self.front == other.front
            && self.back == State::Body
            && other.back == State::Body
        {
            // possible future improvement: this could bail out earlier if there were a
            // reverse memcmp/bcmp comparing back to front
            if self.path == other.path {
                return true;
            }
        }

        // compare back to front since absolute paths often share long prefixes
        Iterator::eq(self.clone().rev(), other.clone().rev())
    }
}

impl Eq for Components<'_> {}

impl<'a> PartialOrd for Components<'a> {
    #[inline]
    fn partial_cmp(&self, other: &Components<'a>) -> Option<cmp::Ordering> {
        Some(compare_components(self.clone(), other.clone()))
    }
}

impl Ord for Components<'_> {
    #[inline]
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        compare_components(self.clone(), other.clone())
    }
}

fn compare_components(mut left: Components<'_>, mut right: Components<'_>) -> cmp::Ordering {
    // Fast path for long shared prefixes
    //
    // - compare raw bytes to find first mismatch
    // - backtrack to find separator before mismatch to avoid ambiguous parsings of '.' or '..' characters
    // - if found update state to only do a component-wise comparison on the remainder,
    //   otherwise do it on the full path
    //
    // The fast path isn't taken for paths with a PrefixComponent to avoid backtracking into
    // the middle of one
    if left.front == right.front {
        // possible future improvement: a [u8]::first_mismatch simd implementation
        let first_difference = match left.path.iter().zip(right.path).position(|(&a, &b)| a != b) {
            None if left.path.len() == right.path.len() => return cmp::Ordering::Equal,
            None => left.path.len().min(right.path.len()),
            Some(diff) => diff,
        };

        if let Some(previous_sep) = left.path[..first_difference]
            .iter()
            .rposition(|&b| is_sep_byte(b))
        {
            let mismatched_component_start = previous_sep + 1;
            left.path = &left.path[mismatched_component_start..];
            left.front = State::Body;
            right.path = &right.path[mismatched_component_start..];
            right.front = State::Body;
        }
    }

    Iterator::cmp(left, right)
}

impl<'a> Iterator for Components<'a> {
    type Item = Component<'a>;

    fn next(&mut self) -> Option<Self::Item> {
        while !self.finished() {
            match self.front {
                State::StartDir => {
                    self.front = State::Body;
                    if self.has_root {
                        debug_assert!(!self.path.is_empty());
                        self.path = &self.path[1..];
                        return Some(Component::RootDir);
                    } else if self.include_cur_dir() {
                        debug_assert!(!self.path.is_empty());
                        self.path = &self.path[1..];
                        return Some(Component::CurDir);
                    }
                }
                State::Body if !self.path.is_empty() => {
                    let (size, comp) = self.parse_next_component();
                    self.path = &self.path[size..];
                    if comp.is_some() {
                        return comp;
                    }
                }
                State::Body => {
                    self.front = State::Done;
                }
                State::Done => unreachable!(),
            }
        }
        None
    }
}

impl<'a> DoubleEndedIterator for Components<'a> {
    fn next_back(&mut self) -> Option<Self::Item> {
        while !self.finished() {
            match self.back {
                State::Body if self.path.len() > self.len_before_body() => {
                    let (size, comp) = self.parse_next_component_back();
                    self.path = &self.path[..self.path.len() - size];
                    if comp.is_some() {
                        return comp;
                    }
                }
                State::Body => {
                    self.back = State::StartDir;
                }
                State::StartDir => {
                    self.back = State::Done;
                    if self.has_root {
                        self.path = &self.path[..self.path.len() - 1];
                        return Some(Component::RootDir);
                    } else if self.include_cur_dir() {
                        self.path = &self.path[..self.path.len() - 1];
                        return Some(Component::CurDir);
                    }
                }
                State::Done => unreachable!(),
            }
        }
        None
    }
}

impl FusedIterator for Components<'_> {}

pub struct Iter<'a> {
    inner: Components<'a>,
}

impl<'a> Iter<'a> {
    #[must_use]
    #[inline]
    pub fn as_path(&self) -> &'a Path {
        self.inner.as_path()
    }
}

impl AsRef<Path> for Iter<'_> {
    #[inline]
    fn as_ref(&self) -> &Path {
        self.as_path()
    }
}

impl AsRef<str> for Iter<'_> {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_path().as_str()
    }
}

impl<'a> Iterator for Iter<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        self.inner.next().map(Component::as_str)
    }
}

impl<'a> DoubleEndedIterator for Iter<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a str> {
        self.inner.next_back().map(Component::as_str)
    }
}

impl FusedIterator for Iter<'_> {}

/// A slice of a path (akin to [`str`]).
///
/// This type supports a number of operations for inspecting a path, including
/// breaking the path into its components (always separated by '/'),
/// extracting the file name, determining whether the path
/// is absolute, and so on.
///
/// This is an *unsized* type, meaning that it must always be used behind a
/// pointer like `&` or [`Box`]. For an owned version of this type,
/// see [`PathBuf`].
///
/// More details about the overall approach can be found in
/// the [module documentation](self).
///
/// # Examples
///
/// ```
/// use fsync::path::Path;
///
/// let path = Path::new("./foo/bar.txt");
///
/// let parent = path.parent();
/// assert_eq!(parent, Some(Path::new("./foo")));
/// ```
#[repr(transparent)]
pub struct Path {
    inner: str,
}

impl Path {
    unsafe fn from_utf8_unchecked(s: &[u8]) -> &Path {
        Path::new(str::from_utf8_unchecked(s))
    }

    pub fn new<S: AsRef<str> + ?Sized>(path: &S) -> &Path {
        unsafe { &*(path.as_ref() as *const str as *const Path) }
    }

    pub fn root() -> &'static Path {
        Path::new("/")
    }

    /// Yields the underlying [`str`] slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// let s = Path::new("foo.txt").as_str();
    /// assert_eq!(s, "foo.txt");
    /// ```
    pub fn as_str(&self) -> &str {
        &self.inner
    }

    /// Converts a `Path` to an owned [`PathBuf`].
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// let path_buf = Path::new("foo.txt").to_path_buf();
    /// assert_eq!(path_buf, PathBuf::from("foo.txt"));
    /// ```
    pub fn to_path_buf(&self) -> PathBuf {
        PathBuf {
            inner: self.inner.to_string(),
        }
    }

    /// Returns `true` if the `Path` has a root.
    ///
    /// * A path has a root if it begins with `/`.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert!(Path::new("/etc/passwd").has_root());
    /// ```
    #[inline]
    pub fn has_root(&self) -> bool {
        has_root(self.inner.as_bytes())
    }

    /// Returns `true` if the `Path` is absolute, i.e., if it is independent of
    /// the current directory.
    ///
    /// * A path is absolute if it starts with the root, so
    /// `is_absolute` and [`has_root`] are equivalent.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert!(!Path::new("foo.txt").is_absolute());
    /// ```
    ///
    /// [`has_root`]: Path::has_root
    #[inline]
    pub fn is_absolute(&self) -> bool {
        self.has_root()
    }

    /// Returns `true` if the `Path` is relative, i.e., not absolute.
    ///
    /// See [`is_absolute`]'s documentation for more details.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert!(Path::new("foo.txt").is_relative());
    /// ```
    ///
    /// [`is_absolute`]: Path::is_absolute
    #[inline]
    pub fn is_relative(&self) -> bool {
        !self.is_absolute()
    }

    /// Returns this `Path` without the root component if it has one.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert_eq!(Path::new("/foo/bar").without_root(), Path::new("foo/bar"));
    /// assert_eq!(Path::new("foo/bar").without_root(), Path::new("foo/bar"));
    /// ```
    ///
    /// [`has_root`]: Path::has_root
    #[inline]
    pub fn without_root(&self) -> &Path {
        let mut comps = self.components();
        if comps.has_root() {
            comps.next();
        }
        comps.as_path()
    }

    /// Returns `true` if the `Path` is the root (aka "/").
    ///
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert!(Path::new("/").is_root());
    /// assert!(!Path::new("/foo").is_root());
    /// assert!(!Path::new("foo").is_root());
    /// ```
    #[inline]
    pub fn is_root(&self) -> bool {
        let mut comps = self.components();
        let c1 = comps.next();
        let c2 = comps.next();
        (c1, c2) == (Some(Component::RootDir), None)
    }

    /// Returns the `Path` without its final component, if there is one.
    ///
    /// This means it returns `Some("")` for relative paths with one component.
    ///
    /// Returns [`None`] if the path terminates in a root or prefix, or if it's
    /// the empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// let path = Path::new("/foo/bar");
    /// let parent = path.parent().unwrap();
    /// assert_eq!(parent, Path::new("/foo"));
    ///
    /// let grand_parent = parent.parent().unwrap();
    /// assert_eq!(grand_parent, Path::new("/"));
    /// assert_eq!(grand_parent.parent(), None);
    ///
    /// let relative_path = Path::new("foo/bar");
    /// let parent = relative_path.parent();
    /// assert_eq!(parent, Some(Path::new("foo")));
    /// let grand_parent = parent.and_then(Path::parent);
    /// assert_eq!(grand_parent, Some(Path::new("")));
    /// let great_grand_parent = grand_parent.and_then(Path::parent);
    /// assert_eq!(great_grand_parent, None);
    /// ```
    #[must_use]
    pub fn parent(&self) -> Option<&Path> {
        let mut comps = self.components();
        let comp = comps.next_back();
        comp.and_then(|p| match p {
            Component::Normal(_) | Component::CurDir | Component::ParentDir => {
                Some(comps.as_path())
            }
            _ => None,
        })
    }

    /// Returns the final component of the `Path`, if there is one.
    ///
    /// If the path is a normal file, this is the file name. If it's the path of a directory, this
    /// is the directory name.
    ///
    /// Returns [`None`] if the path terminates in `..`.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert_eq!(Some("bin"), Path::new("/usr/bin/").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("tmp/foo.txt").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("foo.txt/.").file_name());
    /// assert_eq!(Some("foo.txt"), Path::new("foo.txt/.//").file_name());
    /// assert_eq!(None, Path::new("foo.txt/..").file_name());
    /// assert_eq!(None, Path::new("/").file_name());
    /// ```
    #[must_use]
    pub fn file_name(&self) -> Option<&str> {
        self.components().next_back().and_then(|p| match p {
            Component::Normal(p) => Some(p),
            _ => None,
        })
    }

    /// Produces an iterator over the [`Component`]s of the path.
    ///
    /// When parsing the path, there is a small amount of normalization:
    ///
    /// * Repeated separators are ignored, so `a/b` and `a//b` both have
    ///   `a` and `b` as components.
    ///
    /// * Occurrences of `.` are normalized away, except if they are at the
    ///   beginning of the path. For example, `a/./b`, `a/b/`, `a/b/.` and
    ///   `a/b` all have `a` and `b` as components, but `./a/b` starts with
    ///   an additional [`CurDir`] component.
    ///
    /// * A trailing slash is normalized away, `/a/b` and `/a/b/` are equivalent.
    ///
    /// Note that no other normalization takes place; in particular, `a/c`
    /// and `a/b/../c` are distinct, to account for the possibility that `b`
    /// is a symbolic link (so its parent isn't `a`).
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, Component};
    ///
    /// let mut components = Path::new("/tmp/foo.txt").components();
    ///
    /// assert_eq!(components.next(), Some(Component::RootDir));
    /// assert_eq!(components.next(), Some(Component::Normal("tmp")));
    /// assert_eq!(components.next(), Some(Component::Normal("foo.txt")));
    /// assert_eq!(components.next(), None)
    /// ```
    ///
    /// [`CurDir`]: Component::CurDir
    pub fn components(&self) -> Components<'_> {
        Components {
            path: self.inner.as_bytes(),
            has_root: has_root(self.inner.as_bytes()),
            front: State::StartDir,
            back: State::Body,
        }
    }

    /// Produces an iterator over the path's components viewed as [`str`]
    /// slices.
    ///
    /// For more information about the particulars of how the path is separated
    /// into components, see [`components`].
    ///
    /// [`components`]: Path::components
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{self, Path};
    ///
    /// let mut it = Path::new("/tmp/foo.txt").iter();
    /// assert_eq!(it.next(), Some("/"));
    /// assert_eq!(it.next(), Some("tmp"));
    /// assert_eq!(it.next(), Some("foo.txt"));
    /// assert_eq!(it.next(), None)
    /// ```
    #[inline]
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            inner: self.components(),
        }
    }

    /// Extracts the stem (non-extension) portion of [`self.file_name`].
    ///
    /// [`self.file_name`]: Path::file_name
    ///
    /// The stem is:
    ///
    /// * [`None`], if there is no file name;
    /// * The entire file name if there is no embedded `.`;
    /// * The entire file name if the file name begins with `.` and has no other `.`s within;
    /// * Otherwise, the portion of the file name before the final `.`
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert_eq!("foo", Path::new("foo.rs").file_stem().unwrap());
    /// assert_eq!("foo.tar", Path::new("foo.tar.gz").file_stem().unwrap());
    /// ```
    ///
    /// # See Also
    /// This method is similar to [`Path::file_prefix`], which extracts the portion of the file name
    /// before the *first* `.`
    ///
    /// [`Path::file_prefix`]: Path::file_prefix
    ///
    #[must_use]
    pub fn file_stem(&self) -> Option<&str> {
        self.file_name()
            .map(rsplit_file_at_dot)
            .and_then(|(before, after)| before.or(after))
    }

    /// Extracts the extension (without the leading dot) of [`self.file_name`], if possible.
    ///
    /// The extension is:
    ///
    /// * [`None`], if there is no file name;
    /// * [`None`], if there is no embedded `.`;
    /// * [`None`], if the file name begins with `.` and has no other `.`s within;
    /// * Otherwise, the portion of the file name after the final `.`
    ///
    /// [`self.file_name`]: fsync::path::Path::file_name
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::Path;
    ///
    /// assert_eq!("rs", Path::new("/foo.rs").extension().unwrap());
    /// assert_eq!("gz", Path::new("/foo.tar.gz").extension().unwrap());
    /// assert!(Path::new("foo").extension().is_none());
    /// assert!(Path::new("/dir/.git").extension().is_none());
    /// assert_eq!("json", Path::new("/dir/.config.json").extension().unwrap());
    /// ```
    #[must_use]
    pub fn extension(&self) -> Option<&str> {
        self.file_name()
            .map(rsplit_file_at_dot)
            .and_then(|(before, after)| before.and(after))
    }
    /// Creates an owned [`PathBuf`] with `path` adjoined to `self`.
    ///
    /// If `path` is absolute, it replaces the current path.
    ///
    /// See [`PathBuf::push`] for more details on what it means to adjoin a path.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// assert_eq!(Path::new("/etc").join("passwd"), PathBuf::from("/etc/passwd"));
    /// assert_eq!(Path::new("/etc").join("/bin/sh"), PathBuf::from("/bin/sh"));
    /// ```
    pub fn join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self._join(path.as_ref())
    }

    fn _join(&self, path: &Path) -> PathBuf {
        let mut buf = self.to_path_buf();
        buf.push(path);
        buf
    }

    /// Creates an owned [`PathBuf`] like `self` but with the given extension.
    ///
    /// See [`PathBuf::set_extension`] for more details.
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// let path = Path::new("foo.rs");
    /// assert_eq!(path.with_extension("txt"), PathBuf::from("foo.txt"));
    ///
    /// let path = Path::new("foo.tar.gz");
    /// assert_eq!(path.with_extension(""), PathBuf::from("foo.tar"));
    /// assert_eq!(path.with_extension("xz"), PathBuf::from("foo.tar.xz"));
    /// assert_eq!(path.with_extension("").with_extension("txt"), PathBuf::from("foo.txt"));
    /// ```
    pub fn with_extension<S: AsRef<str>>(&self, extension: S) -> PathBuf {
        self._with_extension(extension.as_ref())
    }

    fn _with_extension(&self, extension: &str) -> PathBuf {
        let self_len = self.as_str().len();
        let self_bytes = self.as_str().as_bytes();

        let (new_capacity, slice_to_copy) = match self.extension() {
            None => {
                // Enough capacity for the extension and the dot
                let capacity = self_len + extension.len() + 1;
                let whole_path = self_bytes.iter();
                (capacity, whole_path)
            }
            Some(previous_extension) => {
                let capacity = self_len + extension.len() - previous_extension.len();
                let path_till_dot = self_bytes[..self_len - previous_extension.len()].iter();
                (capacity, path_till_dot)
            }
        };

        let mut new_path = PathBuf::with_capacity(new_capacity);
        new_path.as_mut_vec().extend(slice_to_copy);
        new_path.set_extension(extension);
        new_path
    }

    /// Normalizes a path. That is, it removes '.' and '..' components, as well as separator duplicates.
    /// Returns Err if the path can't be normalized (because of '..' going before the root)
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// assert_eq!(Path::new("/some//path").normalize().unwrap(), Path::new("/some/path"));
    /// assert_eq!(Path::new("/some/./path/.").normalize().unwrap(), Path::new("/some/path"));
    /// assert_eq!(Path::new("/some//other/../path").normalize().unwrap(), Path::new("/some/path"));
    /// assert!(Path::new("/../path").normalize().is_err());
    /// ```
    pub fn normalize(&self) -> Result<PathBuf, NormalizeError> {
        let mut res = PathBuf::new();
        for c in self.components() {
            match c {
                Component::RootDir | Component::Normal(_) => res.push(c.as_str()),
                Component::CurDir => (),
                Component::ParentDir => {
                    if !res.pop() {
                        return Err(NormalizeError(self.to_owned()));
                    }
                }
            }
        }
        Ok(res)
    }

    /// Checks whether self is an ancestor of the other path
    ///
    /// # Examples
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// assert!(Path::new("/a/b").is_ancestor_of(Path::new("/a/b/c")));
    /// assert!(Path::new("/a").is_ancestor_of(Path::new("/a/b/c")));
    ///
    /// assert!(!Path::new("/a/b/c").is_ancestor_of(Path::new("/a/b/d")));
    /// assert!(!Path::new("/a/b/c").is_ancestor_of(Path::new("/a/b")));
    /// assert!(!Path::new("/a/b").is_ancestor_of(Path::new("/a/b")));
    /// assert!(!Path::new("/b/a").is_ancestor_of(Path::new("/a/b/c")));
    /// ```
    pub fn is_ancestor_of<P: AsRef<Path>>(&self, other: P) -> bool {
        let mut other = other.as_ref().components();
        let mut otherc = other.next();
        for selfc in self.components() {
            if let Some(otherc) = otherc {
                if selfc != otherc {
                    return false;
                }
            } else {
                return false;
            }
            otherc = other.next();
        }
        return otherc.is_some();
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct PathBuf {
    inner: String,
}

impl PathBuf {
    #[inline]
    fn as_mut_vec(&mut self) -> &mut Vec<u8> {
        unsafe { &mut *(self as *mut PathBuf as *mut Vec<u8>) }
    }

    pub fn new() -> PathBuf {
        PathBuf {
            inner: String::new(),
        }
    }

    pub fn root() -> PathBuf {
        PathBuf {
            inner: String::from("/"),
        }
    }

    /// Creates a new `PathBuf` with a given capacity used to create the
    /// internal [`String`]. See [`with_capacity`] defined on [`String`].
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::PathBuf;
    ///
    /// let mut path = PathBuf::with_capacity(20);
    /// let capacity = path.capacity();
    ///
    /// // This push is done without reallocating
    /// path.push("/foo/txt.rs");
    ///
    /// assert_eq!(capacity, path.capacity());
    /// ```
    ///
    /// [`with_capacity`]: String::with_capacity
    #[must_use]
    #[inline]
    pub fn with_capacity(capacity: usize) -> PathBuf {
        PathBuf {
            inner: String::with_capacity(capacity),
        }
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.capacity()
    }

    pub fn as_path(&self) -> &Path {
        Path::new(self.inner.as_str())
    }

    pub fn into_string(self) -> String {
        self.inner
    }

    /// Updates [`self.extension`] to `Some(extension)` or to `None` if
    /// `extension` is empty.
    ///
    /// Returns `false` and does nothing if [`self.file_name`] is [`None`],
    /// returns `true` and updates the extension otherwise.
    ///
    /// If [`self.extension`] is [`None`], the extension is added; otherwise
    /// it is replaced.
    ///
    /// If `extension` is the empty string, [`self.extension`] will be [`None`]
    /// afterwards, not `Some("")`.
    ///
    /// # Caveats
    ///
    /// The new `extension` may contain dots and will be used in its entirety,
    /// but only the part after the final dot will be reflected in
    /// [`self.extension`].
    ///
    /// If the file stem contains internal dots and `extension` is empty, part
    /// of the old file stem will be considered the new [`self.extension`].
    ///
    /// See the examples below.
    ///
    /// [`self.file_name`]: Path::file_name
    /// [`self.extension`]: Path::extension
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// let mut p = PathBuf::from("/feel/the");
    ///
    /// p.set_extension("force");
    /// assert_eq!(Path::new("/feel/the.force"), p.as_path());
    ///
    /// p.set_extension("dark.side");
    /// assert_eq!(Path::new("/feel/the.dark.side"), p.as_path());
    ///
    /// p.set_extension("cookie");
    /// assert_eq!(Path::new("/feel/the.dark.cookie"), p.as_path());
    ///
    /// p.set_extension("");
    /// assert_eq!(Path::new("/feel/the.dark"), p.as_path());
    ///
    /// p.set_extension("");
    /// assert_eq!(Path::new("/feel/the"), p.as_path());
    ///
    /// p.set_extension("");
    /// assert_eq!(Path::new("/feel/the"), p.as_path());
    /// ```
    pub fn set_extension<S: AsRef<str>>(&mut self, extension: S) -> bool {
        self._set_extension(extension.as_ref())
    }

    fn _set_extension(&mut self, extension: &str) -> bool {
        let file_stem = match self.file_stem() {
            None => return false,
            Some(f) => f,
        };

        // truncate until right after the file stem
        let end_file_stem = file_stem[file_stem.len()..].as_ptr() as usize;
        let start = self.inner.as_bytes().as_ptr() as usize;
        let v = self.as_mut_vec();
        v.truncate(end_file_stem.wrapping_sub(start));

        // add the new extension, if any
        let new = extension.as_bytes();
        if !new.is_empty() {
            v.reserve_exact(new.len() + 1);
            v.push(b'.');
            v.extend_from_slice(new);
        }

        true
    }

    /// Extends `self` with `path`.
    ///
    /// If `path` is absolute, it replaces the current path.
    ///
    /// Consider using [`Path::join`] if you need a new `PathBuf` instead of
    /// using this function on a cloned `PathBuf`.
    ///
    /// # Examples
    ///
    /// Pushing a relative path extends the existing path:
    ///
    /// ```
    /// use fsync::path::PathBuf;
    ///
    /// let mut path = PathBuf::from("/tmp");
    /// path.push("file.bk");
    /// assert_eq!(path, PathBuf::from("/tmp/file.bk"));
    /// ```
    ///
    /// Pushing an absolute path replaces the existing path:
    ///
    /// ```
    /// use fsync::path::PathBuf;
    ///
    /// let mut path = PathBuf::from("/tmp");
    /// path.push("/etc");
    /// assert_eq!(path, PathBuf::from("/etc"));
    /// ```
    pub fn push<P: AsRef<Path>>(&mut self, path: P) {
        self._push(path.as_ref())
    }

    fn _push(&mut self, path: &Path) {
        // in general, a separator is needed if the rightmost byte is not a separator
        let need_sep = self
            .inner
            .as_bytes()
            .last()
            .map(|c| !is_sep_byte(*c))
            .unwrap_or(false);

        // absolute `path` replaces `self`
        if path.is_absolute() {
            self.inner.truncate(0);

        // `path` is a pure relative path
        } else if need_sep {
            self.inner.push(SEPARATOR);
        }

        self.inner.push_str(path.as_str());
    }

    /// Truncates `self` to [`self.parent`].
    ///
    /// Returns `false` and does nothing if [`self.parent`] is [`None`].
    /// Otherwise, returns `true`.
    ///
    /// [`self.parent`]: Path::parent
    ///
    /// # Examples
    ///
    /// ```
    /// use fsync::path::{Path, PathBuf};
    ///
    /// let mut p = PathBuf::from("/spirited/away.rs");
    ///
    /// p.pop();
    /// assert_eq!(Path::new("/spirited"), p);
    /// p.pop();
    /// assert_eq!(Path::new("/"), p);
    /// ```
    pub fn pop(&mut self) -> bool {
        match self.parent().map(|p| p.as_str().len()) {
            Some(len) => {
                self.as_mut_vec().truncate(len);
                true
            }
            None => false,
        }
    }
}

impl AsRef<str> for Path {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for PathBuf {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<Path> for PathBuf {
    #[inline]
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<Path> for Path {
    #[inline]
    fn as_ref(&self) -> &Path {
        self
    }
}

impl AsRef<Path> for str {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for String {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl AsRef<Path> for borrow::Cow<'_, str> {
    #[inline]
    fn as_ref(&self) -> &Path {
        Path::new(self)
    }
}

impl TryFrom<std::path::PathBuf> for PathBuf {
    type Error = std::string::FromUtf8Error;

    /// Try to convert a `std::path::PathBuf` into a `PathBuf`.
    /// This is mostly useful for symlink targets, as normally
    /// very few interactions are needed between `fsync::path`
    /// and the actual file system.
    fn try_from(path: std::path::PathBuf) -> Result<PathBuf, Self::Error> {
        let bytes = path.into_os_string().into_encoded_bytes();
        let utf8 = String::from_utf8(bytes)?;
        Ok(PathBuf::from(utf8))
    }
}

impl<'a> TryFrom<&'a std::path::Path> for &'a Path {
    type Error = std::str::Utf8Error;

    /// Try to convert a `&std::path::Path` into a `&Path`.
    /// This is mostly useful for symlink targets, as normally
    /// very few interactions are needed between `fsync::path`
    /// and the actual file system.
    fn try_from(path: &'a std::path::Path) -> Result<&'a Path, Self::Error> {
        let bytes = path.as_os_str().as_encoded_bytes();
        let utf8 = std::str::from_utf8(bytes)?;
        Ok(Path::new(utf8))
    }
}

impl fmt::Debug for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Path(")?;
        fmt::Debug::fmt(&self.inner, f)?;
        f.write_str(")")
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl Default for &Path {
    fn default() -> Self {
        Path::new("")
    }
}

impl From<String> for PathBuf {
    fn from(value: String) -> Self {
        PathBuf { inner: value }
    }
}

impl<T: ?Sized + AsRef<str>> From<&T> for PathBuf {
    /// Converts a borrowed [`str`] to a [`PathBuf`].
    ///
    /// Allocates a [`PathBuf`] and copies the data into it.
    #[inline]
    fn from(s: &T) -> PathBuf {
        PathBuf::from(s.as_ref().to_string())
    }
}

impl fmt::Debug for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PathBuf(")?;
        fmt::Debug::fmt(&self.inner, f)?;
        f.write_str(")")
    }
}

impl fmt::Display for PathBuf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.inner)
    }
}

impl ops::Deref for PathBuf {
    type Target = Path;

    fn deref(&self) -> &Path {
        self.as_path()
    }
}

impl borrow::Borrow<Path> for PathBuf {
    fn borrow(&self) -> &Path {
        self.as_path()
    }
}

impl Clone for PathBuf {
    #[inline]
    fn clone(&self) -> Self {
        PathBuf {
            inner: self.inner.clone(),
        }
    }

    #[inline]
    fn clone_from(&mut self, source: &Self) {
        self.inner.clone_from(&source.inner)
    }
}

impl From<PathBuf> for String {
    /// Converts a [`PathBuf`] into an [`OsString`]
    ///
    /// This conversion does not allocate or copy memory.
    #[inline]
    fn from(path_buf: PathBuf) -> String {
        path_buf.into_string()
    }
}

impl str::FromStr for PathBuf {
    type Err = core::convert::Infallible;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(PathBuf::from(s))
    }
}

impl<P: AsRef<Path>> FromIterator<P> for PathBuf {
    fn from_iter<I: IntoIterator<Item = P>>(iter: I) -> PathBuf {
        let mut buf = PathBuf::new();
        buf.extend(iter);
        buf
    }
}

impl<P: AsRef<Path>> Extend<P> for PathBuf {
    fn extend<I: IntoIterator<Item = P>>(&mut self, iter: I) {
        iter.into_iter().for_each(move |p| self.push(p.as_ref()));
    }
}

impl<'a> From<&'a Path> for borrow::Cow<'a, Path> {
    /// Creates a clone-on-write pointer from a reference to
    /// [`Path`].
    ///
    /// This conversion does not clone or allocate.
    #[inline]
    fn from(s: &'a Path) -> borrow::Cow<'a, Path> {
        borrow::Cow::Borrowed(s)
    }
}

impl<'a> From<PathBuf> for borrow::Cow<'a, Path> {
    /// Creates a clone-on-write pointer from an owned
    /// instance of [`PathBuf`].
    ///
    /// This conversion does not clone or allocate.
    #[inline]
    fn from(s: PathBuf) -> borrow::Cow<'a, Path> {
        borrow::Cow::Owned(s)
    }
}

impl<'a> From<&'a PathBuf> for borrow::Cow<'a, Path> {
    /// Creates a clone-on-write pointer from a reference to
    /// [`PathBuf`].
    ///
    /// This conversion does not clone or allocate.
    #[inline]
    fn from(p: &'a PathBuf) -> borrow::Cow<'a, Path> {
        borrow::Cow::Borrowed(p.as_path())
    }
}

impl<'a> From<borrow::Cow<'a, Path>> for PathBuf {
    /// Converts a clone-on-write pointer to an owned path.
    ///
    /// Converting from a `borrow::Cow::Owned` does not clone or allocate.
    #[inline]
    fn from(p: borrow::Cow<'a, Path>) -> Self {
        p.into_owned()
    }
}

impl ToOwned for Path {
    type Owned = PathBuf;
    #[inline]
    fn to_owned(&self) -> PathBuf {
        self.to_path_buf()
    }
    #[inline]
    fn clone_into(&self, target: &mut PathBuf) {
        self.inner.clone_into(&mut target.inner);
    }
}

impl PartialEq for PathBuf {
    #[inline]
    fn eq(&self, other: &PathBuf) -> bool {
        self.components() == other.components()
    }
}

impl hash::Hash for PathBuf {
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        self.as_path().hash(h)
    }
}

impl Eq for PathBuf {}

impl PartialOrd for PathBuf {
    #[inline]
    fn partial_cmp(&self, other: &PathBuf) -> Option<cmp::Ordering> {
        Some(compare_components(self.components(), other.components()))
    }
}

impl Ord for PathBuf {
    #[inline]
    fn cmp(&self, other: &PathBuf) -> cmp::Ordering {
        compare_components(self.components(), other.components())
    }
}

impl PartialEq for Path {
    #[inline]
    fn eq(&self, other: &Path) -> bool {
        self.components() == other.components()
    }
}

impl hash::Hash for Path {
    fn hash<H: hash::Hasher>(&self, h: &mut H) {
        let bytes = self.inner.as_bytes();

        let mut component_start = 0;
        let mut bytes_hashed = 0;

        for i in 0..bytes.len() {
            let is_sep = is_sep_byte(bytes[i]);
            if is_sep {
                if i > component_start {
                    let to_hash = &bytes[component_start..i];
                    h.write(to_hash);
                    bytes_hashed += to_hash.len();
                }

                // skip over separator and optionally a following CurDir item
                // since components() would normalize these away.
                component_start = i + 1;

                let tail = &bytes[component_start..];

                component_start += match tail {
                    [b'.'] => 1,
                    [b'.', sep, ..] if is_sep_byte(*sep) => 1,
                    _ => 0,
                };
            }
        }

        if component_start < bytes.len() {
            let to_hash = &bytes[component_start..];
            h.write(to_hash);
            bytes_hashed += to_hash.len();
        }

        h.write_usize(bytes_hashed);
    }
}

impl Eq for Path {}

impl PartialOrd for Path {
    #[inline]
    fn partial_cmp(&self, other: &Path) -> Option<cmp::Ordering> {
        Some(compare_components(self.components(), other.components()))
    }
}

impl Ord for Path {
    #[inline]
    fn cmp(&self, other: &Path) -> cmp::Ordering {
        compare_components(self.components(), other.components())
    }
}
impl<'a> IntoIterator for &'a PathBuf {
    type Item = &'a str;
    type IntoIter = Iter<'a>;
    #[inline]
    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

impl<'a> IntoIterator for &'a Path {
    type Item = &'a str;
    type IntoIter = Iter<'a>;
    #[inline]
    fn into_iter(self) -> Iter<'a> {
        self.iter()
    }
}

macro_rules! impl_cmp {
    (<$($life:lifetime),*> $lhs:ty, $rhs: ty) => {
        impl<$($life),*> PartialEq<$rhs> for $lhs {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool {
                <Path as PartialEq>::eq(self, other)
            }
        }

        impl<$($life),*> PartialEq<$lhs> for $rhs {
            #[inline]
            fn eq(&self, other: &$lhs) -> bool {
                <Path as PartialEq>::eq(self, other)
            }
        }

        impl<$($life),*> PartialOrd<$rhs> for $lhs {
            #[inline]
            fn partial_cmp(&self, other: &$rhs) -> Option<cmp::Ordering> {
                <Path as PartialOrd>::partial_cmp(self, other)
            }
        }

        impl<$($life),*> PartialOrd<$lhs> for $rhs {
            #[inline]
            fn partial_cmp(&self, other: &$lhs) -> Option<cmp::Ordering> {
                <Path as PartialOrd>::partial_cmp(self, other)
            }
        }
    };
}

impl_cmp!(<> PathBuf, Path);
impl_cmp!(<'a> PathBuf, &'a Path);
impl_cmp!(<'a> borrow::Cow<'a, Path>, Path);
impl_cmp!(<'a, 'b> borrow::Cow<'a, Path>, &'b Path);
impl_cmp!(<'a> borrow::Cow<'a, Path>, PathBuf);

macro_rules! impl_cmp_str {
    (<$($life:lifetime),*> $lhs:ty, $rhs: ty) => {
        impl<$($life),*> PartialEq<$rhs> for $lhs {
            #[inline]
            fn eq(&self, other: &$rhs) -> bool {
                <Path as PartialEq>::eq(self, other.as_ref())
            }
        }

        impl<$($life),*> PartialEq<$lhs> for $rhs {
            #[inline]
            fn eq(&self, other: &$lhs) -> bool {
                <Path as PartialEq>::eq(self.as_ref(), other)
            }
        }

        impl<$($life),*> PartialOrd<$rhs> for $lhs {
            #[inline]
            fn partial_cmp(&self, other: &$rhs) -> Option<cmp::Ordering> {
                <Path as PartialOrd>::partial_cmp(self, other.as_ref())
            }
        }

        impl<$($life),*> PartialOrd<$lhs> for $rhs {
            #[inline]
            fn partial_cmp(&self, other: &$lhs) -> Option<cmp::Ordering> {
                <Path as PartialOrd>::partial_cmp(self.as_ref(), other)
            }
        }
    };
}

impl_cmp_str!(<> PathBuf, str);
// impl_cmp_str!(<'a> PathBuf, &'a str);
// impl_cmp_str!(<'a> PathBuf, borrow::Cow<'a, str>);
// impl_cmp_str!(<> PathBuf, String);
// impl_cmp_str!(<> Path, str);
// impl_cmp_str!(<'a> Path, &'a str);
// impl_cmp_str!(<'a> Path, borrow::Cow<'a, str>);
// impl_cmp_str!(<> Path, String);
// impl_cmp_str!(<'a> &'a Path, str);
// impl_cmp_str!(<'a, 'b> &'a Path, borrow::Cow<'b, str>);
// impl_cmp_str!(<'a> &'a Path, String);
// impl_cmp_str!(<'a> borrow::Cow<'a, Path>, str);
// impl_cmp_str!(<'a, 'b> borrow::Cow<'a, Path>, &'b str);
// impl_cmp_str!(<'a> borrow::Cow<'a, Path>, String);

impl TypeDef for PathBuf {
    const INFO: type_expr::TypeInfo = type_expr::TypeInfo::Native(type_expr::NativeTypeInfo {
        r#ref: type_expr::TypeExpr::ident(type_expr::Ident("string")),
    });
}
