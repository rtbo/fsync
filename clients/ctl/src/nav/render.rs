//! Rendering module for the navigator.
//! For each rendered frame, each character of the screen buffer is written
//! once and only once. This is done without clearing the screen first
//! to avoid flickering.
use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, PrintStyledContent, Stylize},
};
use fsync::tree::{Entry, EntryNode};

use crate::utils;

const LOCAL_COLOR: Color = Color::Reset;
const REMOTE_COLOR: Color = Color::Cyan;
const NODE_COLOR: Color = Color::Magenta;
const CONFLICT_COLOR: Color = Color::Red;
const SYNC_COLOR: Color = Color::Green;

fn entry_print_path(entry: &Entry) -> String {
    let path = entry.path().to_string();
    if entry.is_safe_dir() && !entry.path().is_root() {
        format!("{path}/")
    } else {
        path
    }
}

fn entry_print_name(entry: &Entry, max_width: Option<u16>) -> String {
    let name = {
        let name = entry.path().file_name().unwrap_or_default().to_string();
        if entry.is_safe_dir() {
            format!("{name}/")
        } else {
            name
        }
    };
    if let Some(max_width) = max_width {
        elided(name, max_width)
    } else {
        name
    }
}

fn elided(name: String, max_width: u16) -> String {
    assert!(max_width >= 5);
    if name.width() > max_width {
        let start_width = max_width / 2 - 1;
        let end_width = max_width - start_width - 3;
        let start: String = name.chars().take(start_width as usize).collect();
        let end: String = name.chars().rev().take(end_width as usize).collect();
        format!("{start}...{end}")
    } else {
        name
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tag {
    color: Color,
    tag: char,
    desc: &'static str,
    desc_short: &'static str,
}

impl Tag {
    // synchronized tag
    const fn sync() -> Self {
        Tag {
            color: SYNC_COLOR,
            tag: 'S',
            desc: "Synchronized",
            desc_short: "Sync",
        }
    }

    // local only tag
    const fn local() -> Self {
        Tag {
            color: LOCAL_COLOR,
            tag: 'L',
            desc: "Local only",
            desc_short: "Local",
        }
    }

    // remote only tag
    const fn remote() -> Self {
        Tag {
            color: REMOTE_COLOR,
            tag: 'R',
            desc: "Remote only",
            desc_short: "Remote",
        }
    }

    // conflict tag
    const fn conflict() -> Self {
        Tag {
            color: CONFLICT_COLOR,
            tag: 'C',
            desc: "Conflict",
            desc_short: "Conflict",
        }
    }

    fn print(&self) -> PrintStyledContent<char> {
        PrintStyledContent(self.tag.with(self.color))
    }
}

impl From<&Entry> for Tag {
    fn from(value: &Entry) -> Self {
        match value {
            Entry::Local(..) => Tag::local(),
            Entry::Remote(..) => Tag::remote(),
            Entry::Sync {
                conflict: Some(_), ..
            } => Tag::conflict(),
            Entry::Sync { conflict: None, .. } => Tag::sync(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u16,
    pub height: u16,
}

impl From<(u16, u16)> for Size {
    fn from(value: (u16, u16)) -> Self {
        Self {
            width: value.0,
            height: value.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pos {
    pub x: u16,
    pub y: u16,
}

impl Pos {
    pub fn move_to(&self) -> MoveTo {
        MoveTo(self.x, self.y)
    }
}

impl From<(u16, u16)> for Pos {
    fn from(value: (u16, u16)) -> Self {
        Self {
            x: value.0,
            y: value.1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub top_left: Pos,
    pub size: Size,
}

impl Rect {
    pub fn width(&self) -> u16 {
        self.size.width
    }

    pub fn height(&self) -> u16 {
        self.size.height
    }

    pub fn right(&self) -> u16 {
        self.top_left.x + self.width()
    }

    pub fn abs_pos(&self, pos: Pos) -> Pos {
        Pos {
            x: self.top_left.x + pos.x,
            y: self.top_left.y + pos.y,
        }
    }

    pub fn move_to(&self, pos: Pos) -> MoveTo {
        self.abs_pos(pos).move_to()
    }

    pub fn with_height(&self, h: u16) -> Rect {
        Rect {
            top_left: self.top_left,
            size: Size {
                width: self.width(),
                height: h,
            },
        }
    }

    pub fn crop_left(&self, w: u16) -> Rect {
        Rect {
            top_left: Pos {
                x: self.top_left.x + w,
                y: self.top_left.y,
            },
            size: Size {
                width: self.width() - w,
                height: self.height(),
            },
        }
    }

    pub fn crop_right(&self, w: u16) -> Rect {
        Rect {
            top_left: self.top_left,
            size: Size {
                width: self.width() - w,
                height: self.height(),
            },
        }
    }

    pub fn crop_top(&self, h: u16) -> Rect {
        Rect {
            top_left: Pos {
                x: self.top_left.x,
                y: self.top_left.y + h,
            },
            size: Size {
                width: self.width(),
                height: self.height() - h,
            },
        }
    }
}

trait Width {
    fn width(&self) -> u16;
}

impl Width for Size {
    fn width(&self) -> u16 {
        self.width
    }
}

impl Width for Rect {
    fn width(&self) -> u16 {
        self.size.width
    }
}

impl Width for str {
    fn width(&self) -> u16 {
        self.chars().count() as u16
    }
}

impl Width for String {
    fn width(&self) -> u16 {
        self.chars().count() as u16
    }
}

impl<S: AsRef<str>> Width for Option<S> {
    fn width(&self) -> u16 {
        self.as_ref().map(|s| s.as_ref().width()).unwrap_or(0)
    }
}

/// Animation ticks per second
pub const ANIM_TPS: f32 = 30.0;

/// Animated spinner
#[derive(Default, Debug, Copy, Clone)]
struct Spinner(f32);

impl Spinner {
    const SEQ: &'static [char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

    /// Rounds per second
    const RPS: f32 = 1.0;
    /// Ticks per round
    const TPR: f32 = ANIM_TPS / Self::RPS;

    fn tick(&mut self) {
        debug_assert!(self.0 >= 0.0 && self.0 < 1.0);

        let mut new_value = self.0 + 1.0 / Self::TPR;
        if new_value >= 1.0 {
            new_value -= 1.0;
        }
        self.0 = new_value;
    }

    fn get(&self) -> char {
        debug_assert!(self.0 >= 0.0 && self.0 < 1.0);

        let idx = (self.0 * Self::SEQ.len() as f32) as usize;
        Self::SEQ[idx]
    }
}

#[derive(Debug, Default)]
pub struct State {
    spinner: Spinner,
}

impl State {
    pub fn tick(&mut self) {
        self.spinner.tick();
    }
}

impl super::Navigator {
    /// Render the navigator and returns whether animation is required
    pub async fn render(&self, state: &mut State) -> anyhow::Result<bool> {
        let mut out = io::stdout();

        let max_vp_height = self.size.height - 1;

        let menu_sz = self.menu.max_size();

        let legend_sz = Size {
            width: menu_sz.width,
            height: 5.min(max_vp_height.max(menu_sz.height) - menu_sz.height),
        };

        let menu_viewport = Rect {
            top_left: Pos {
                x: self.size.width - menu_sz.width,
                y: 0,
            },
            size: Size {
                width: menu_sz.width,
                height: (max_vp_height - legend_sz.height).max(menu_sz.height),
            },
        };
        self.menu.render(menu_viewport, self.focus)?;

        let legend_viewport = Rect {
            top_left: Pos {
                x: self.size.width - legend_sz.width,
                y: max_vp_height - legend_sz.height,
            },
            size: legend_sz,
        };
        self.render_legend(legend_viewport)?;

        let viewport = Rect {
            top_left: Pos { x: 0, y: 0 },
            size: Size {
                width: self.size.width - menu_viewport.width(),
                height: max_vp_height,
            },
        };

        let progress = self
            .client
            .progresses(super::ctx(), self.node.path().to_owned())
            .await
            .unwrap()?;

        if self.node.entry().is_safe_dir() {
            self.render_dir(&viewport, state, &progress).await?;
        } else {
            todo!()
        }

        let footer_vp = Rect {
            top_left: Pos {
                x: 0,
                y: self.size.height - 1,
            },
            size: Size {
                width: self.size.width,
                height: 1,
            },
        };
        self.render_stats(&footer_vp, &self.node.stats())?;

        out.flush()?;

        if progress.is_empty() {
            Ok(false)
        } else {
            state.tick();
            Ok(true)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ChildrenScroll {
    height: u16,
    cur_child_pos: u16,
    cur_child_height: u16,
}

impl ChildrenScroll {
    fn has_scroll_bar(&self, vp: &Rect) -> bool {
        self.height > vp.height()
    }

    // compute offset such as to have cur_child in the middle
    fn scroll_offset(&self, vp: &Rect) -> i16 {
        if self.has_scroll_bar(vp) {
            let offset = vp.height() as i16 / 2
                - self.cur_child_pos as i16
                - self.cur_child_height as i16 / 2;
            offset.min(0).max(vp.height() as i16 - self.height as i16)
        } else {
            0
        }
    }

    fn render_bar(&self, vp: &Rect, x: u16, offset: i16) -> anyhow::Result<()> {
        let bar_space = vp.height() - 2;
        let bar_h = (bar_space as f32 * vp.height() as f32 / self.height as f32) as u16;
        let bar_y = (bar_space as f32 * (-offset) as f32 / self.height as f32) as u16;

        let mut out = io::stdout();

        for y in 0..vp.height() {
            let pos = vp.abs_pos(Pos { x, y });
            let sym = match y {
                0 => "ʌ",
                y if y == vp.height() - 1 => "v",
                y if y - 1 < bar_y => " ",
                y if y - 1 <= bar_y + bar_h => "║",
                _ => " ",
            };
            queue!(out, MoveTo(pos.x, pos.y), Print(sym))?;
        }

        Ok(())
    }
}

fn print_progress_bar(width: u16, progress: f32) -> String {
    const CHARS: &[char] = &[' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉', '█'];
    const WHOLE: char = '█';
    const EMPTY: char = ' ';
    const BREAKS: &[f32] = &[
        0.125 * 0.5,
        0.125 * 1.5,
        0.125 * 2.5,
        0.125 * 3.5,
        0.125 * 4.5,
        0.125 * 5.5,
        0.125 * 6.5,
        0.125 * 7.5,
    ];
    let progress = progress.clamp(0.0, 1.0);
    let float_width = progress * width as f32;
    let whole_width = float_width.floor();
    let part = float_width - whole_width;
    let empty_width = width - float_width.ceil() as u16;

    let mut s = String::with_capacity(width as usize);
    for _ in 0..whole_width as usize {
        s.push(WHOLE);
    }
    if part > 0.0 {
        let mut partialc = None;
        for (i, &b) in BREAKS.iter().enumerate() {
            if part < b {
                partialc = Some(CHARS[i]);
                break;
            }
        }
        let partialc = partialc.unwrap_or(WHOLE);
        s.push(partialc);
    }
    for _ in 0..empty_width as usize {
        s.push(EMPTY);
    }
    debug_assert!(s.width() == width);
    s
}

#[cfg(test)]
mod tests {
    use super::print_progress_bar;

    #[test]
    fn test_progress_bar() {
        assert_eq!(print_progress_bar(10, 0.00000), "          ");
        assert_eq!(print_progress_bar(10, 0.50000), "█████     ");
        assert_eq!(print_progress_bar(10, 0.50624), "█████     ");
        assert_eq!(print_progress_bar(10, 0.50626), "█████▏    ");
        assert_eq!(print_progress_bar(10, 0.51250), "█████▏    ");
        assert_eq!(print_progress_bar(10, 0.51874), "█████▏    ");
        assert_eq!(print_progress_bar(10, 0.51876), "█████▎    ");
        assert_eq!(print_progress_bar(10, 0.52500), "█████▎    ");
        assert_eq!(print_progress_bar(10, 0.53124), "█████▎    ");
        assert_eq!(print_progress_bar(10, 0.53126), "█████▍    ");
        assert_eq!(print_progress_bar(10, 0.54374), "█████▍    ");
        assert_eq!(print_progress_bar(10, 0.54376), "█████▌    ");
        assert_eq!(print_progress_bar(10, 0.55624), "█████▌    ");
        assert_eq!(print_progress_bar(10, 1.00000), "██████████");
    }
}

impl super::Navigator {
    fn render_legend(&self, viewport: Rect) -> anyhow::Result<()> {
        let mut out = io::stdout();

        let mut y = 0;

        let mut remain = viewport.height();
        while remain > 0 {
            let (msg, col) = match remain {
                5 => ("Local only", LOCAL_COLOR),
                4 => ("Remote only", REMOTE_COLOR),
                3 => ("Total nodes", NODE_COLOR),
                2 => ("Synchronized", SYNC_COLOR),
                1 => ("Conflicts", CONFLICT_COLOR),
                _ => unreachable!(),
            };

            let x = viewport.width() - msg.width();
            let pos = viewport.abs_pos(Pos { x: 0, y });
            queue!(
                out,
                pos.move_to(),
                Print(" ".repeat(x as usize).as_str()),
                PrintStyledContent(msg.with(col)),
            )?;
            y += 1;
            remain -= 1;
        }
        Ok(())
    }

    fn compute_children_scroll(&self) -> ChildrenScroll {
        let mut height = 0;
        let mut cur_child_pos = 0;
        let mut cur_child_height = 0;
        for idx in 0..self.children.len() {
            let h = self.compute_child_height(idx);
            if idx == self.cur_child {
                cur_child_pos = height;
                cur_child_height = h;
            }
            height += h;
        }
        ChildrenScroll {
            height,
            cur_child_pos,
            cur_child_height,
        }
    }

    async fn render_dir(
        &self,
        viewport: &Rect,
        state: &State,
        progress: &[(fsync::path::PathBuf, fsync::Progress)],
    ) -> anyhow::Result<()> {
        let tag = Tag::from(self.node.entry());
        let node = &self.node;
        let spin = if progress.is_empty() {
            ' '
        } else {
            state.spinner.get()
        };

        let mut out = io::stdout();

        let path = entry_print_path(node.entry());

        let pos = viewport.abs_pos(Pos { x: 0, y: 0 });
        queue!(
            out,
            pos.move_to(),
            tag.print(),
            PrintStyledContent(spin.with(Color::Green)),
            Print(" "),
            Print(&path),
        )?;

        let mut w = path.width() as u16 + 3;

        if self.node.children_have_conflicts() {
            let cf = format!("    [{}]", node.children_conflicts());
            queue!(out, PrintStyledContent(cf.as_str().with(Color::Red)))?;
            w += cf.len() as u16;
        }

        // for long paths, add spaces to ensure a space between the path and the title
        queue!(out, Print("  "))?;
        w += 2;

        if w < viewport.width() {
            queue!(
                out,
                Print(" ".repeat((viewport.width() - w) as usize).as_str(),)
            )?;
        }

        let children_scroll = self.compute_children_scroll();
        let children_vp = {
            let mut vp = viewport.crop_top(1);
            if children_scroll.has_scroll_bar(&vp) {
                vp = vp.crop_right(1);
            }
            vp
        };
        let scroll_offset = children_scroll.scroll_offset(&children_vp);

        if children_scroll.has_scroll_bar(&children_vp) {
            children_scroll.render_bar(&children_vp, children_vp.right(), scroll_offset)?;
        }

        let mut pos = Pos { x: 0, y: 0 };
        for (idx, child) in self.children.iter().enumerate() {
            pos.y += self.render_child_node(
                idx,
                child,
                pos,
                children_vp,
                scroll_offset,
                state,
                progress,
            )?;
        }

        if pos.y < children_vp.height() {
            for y in pos.y..children_vp.height() {
                let y = children_vp.top_left.y + y;
                queue!(
                    out,
                    MoveTo(0, y),
                    Print(" ".repeat(children_vp.width() as usize).as_str()),
                )?;
            }
        }

        Ok(())
    }

    fn compute_child_height(&self, idx: usize) -> u16 {
        if Some(idx) == self.detailed_child {
            4
        } else {
            1
        }
    }

    fn render_child_node(
        &self,
        idx: usize,
        child: &EntryNode,
        pos: Pos,
        viewport: Rect,
        scroll_offset: i16,
        state: &State,
        progress: &[(fsync::path::PathBuf, fsync::Progress)],
    ) -> anyhow::Result<u16> {
        let height = self.compute_child_height(idx);
        let start_y = pos.y as i16 + scroll_offset;

        // start_y is the line showing the child name

        let mut out = io::stdout();

        let is_current = idx == self.cur_child;
        let is_detailed = Some(idx) == self.detailed_child;

        if start_y >= 0 && start_y < viewport.height() as i16 {
            let vp = viewport.with_height(1);

            let mut w = 0;
            let tag = Tag::from(child.entry());

            let mut spin = ' ';
            let mut bar = None;
            for prog in progress {
                if child.path() == prog.0 || child.path().is_ancestor_of(&prog.0) {
                    spin = state.spinner.get();
                    match prog.1 {
                        fsync::Progress::Progress { progress, total } => {
                            let p = progress as f32 / total as f32;
                            bar = Some(format!(" ║{}║ ", print_progress_bar(10, p)));
                        }
                        _ => (),
                    }
                    break;
                }
            }

            let abs_pos = vp.abs_pos(Pos {
                x: pos.x,
                y: start_y as u16,
            });
            queue!(
                out,
                abs_pos.move_to(),
                tag.print(),
                PrintStyledContent(spin.with(Color::Green)),
                Print(" ")
            )?;
            w += 3;

            let path_col = if is_current {
                let sign = if is_detailed { "v " } else { "> " };
                queue!(out, PrintStyledContent(sign.with(Color::Blue)))?;
                Color::Blue
            } else {
                queue!(out, Print("  "))?;
                Color::Reset
            };
            w += 2;

            let mut conflict_str = if child.children_have_conflicts() {
                Some(format!(" [{}]", child.children_conflicts()))
            } else {
                None
            };

            if vp.width() < w + conflict_str.width() + bar.width() {
                bar = None;
            }
            if vp.width() < w + conflict_str.width() + bar.width() {
                conflict_str = None;
            }
            if vp.width() > w + conflict_str.width() + bar.width() {
                let name_max_width = vp.width() - w - conflict_str.width() - bar.width();

                let name = entry_print_name(child.entry(), Some(name_max_width));
                queue!(out, PrintStyledContent(name.as_str().with(path_col)))?;
                w += name.width();

                if let Some(conflict_str) = conflict_str {
                    queue!(
                        out,
                        PrintStyledContent(conflict_str.as_str().with(Color::Red))
                    )?;
                    w += conflict_str.width();
                }

                if w < vp.width() - bar.width() {
                    queue!(
                        out,
                        Print(" ".repeat((vp.width() - w - bar.width()) as usize).as_str(),)
                    )?;
                }

                if let Some(bar) = bar {
                    queue!(out, PrintStyledContent(bar.as_str().with(Color::Green)))?;
                }
            }
        }

        if is_detailed {
            let vp = Rect {
                top_left: viewport.abs_pos(Pos {
                    x: pos.x,
                    y: start_y as u16 + 1,
                }),
                size: Size {
                    width: viewport.width(),
                    height: height - 1,
                },
            };
            let offset = 6u16;
            let blank = " ".repeat(offset as usize);
            for y in 0..vp.height() {
                queue!(out, vp.move_to(Pos { x: 0, y: y as u16 }), Print(&blank),)?;
            }
            let vp = vp.crop_left(offset);
            self.render_child_details(child, &vp)?;
        }

        Ok(height)
    }

    fn render_child_details(&self, child: &EntryNode, viewport: &Rect) -> anyhow::Result<()> {
        let stat = child.stats();
        self.render_stats(&viewport, &stat)?;
        Ok(())
    }

    fn render_stats(&self, viewport: &Rect, stat: &fsync::stat::Tree) -> anyhow::Result<()> {
        debug_assert!(
            viewport.height() == 1 || viewport.height() == 3,
            "only 1 or 3 lines are supported"
        );

        let mut out = io::stdout();

        const SHORT: u16 = 1;
        const MEDIUM: u16 = 2;
        const LONG: u16 = 3;

        fn dir_stat(stat: &fsync::stat::Dir, len_tag: u16) -> String {
            match len_tag {
                SHORT => format!("{data:.1}", data = utils::adjusted_byte(stat.data as _),),
                MEDIUM => format!(
                    "d:{dirs} f:{files} {data:.1}",
                    dirs = stat.dirs,
                    files = stat.files,
                    data = utils::adjusted_byte(stat.data as _),
                ),
                LONG => format!(
                    "dirs:{dirs}  files:{files}  data:{data:.2}",
                    dirs = stat.dirs,
                    files = stat.files,
                    data = utils::adjusted_byte(stat.data as _),
                ),
                _ => unreachable!(),
            }
        }

        fn node_stat(name: &str, stat: i32, len_tag: u16) -> String {
            match len_tag {
                SHORT => format!("{}", stat),
                MEDIUM => format!("{}", stat),
                LONG => format!("{name}:{}", stat),
                _ => unreachable!(),
            }
        }

        let sep1 = " | ";
        let sep3 = "  ";
        let sep = if viewport.height() == 1 { sep1 } else { sep3 };

        let mut len_tag = LONG;

        let (local, remote, nodes, sync, conflicts) = loop {
            let local = dir_stat(&stat.local, len_tag);
            let remote = dir_stat(&stat.remote, len_tag);
            let nodes = node_stat("nodes", stat.node.nodes, len_tag);
            let sync = node_stat("sync", stat.node.sync, len_tag);
            let conflicts = node_stat("conflicts", stat.node.conflicts, len_tag);

            let fits = if viewport.height() == 3 {
                local.width() <= viewport.width()
                    && remote.width() <= viewport.width()
                    && (nodes.width() + sync.width() + conflicts.width() + sep.width() * 2)
                        <= viewport.width()
            } else {
                local.width()
                    + remote.width()
                    + nodes.width()
                    + sync.width()
                    + conflicts.width()
                    + sep.width() * 4
                    <= viewport.width()
            };
            if fits || len_tag == SHORT {
                break (local, remote, nodes, sync, conflicts);
            }
            len_tag -= 1;
        };

        if viewport.height() == 3 {
            let len = local.width();
            queue!(
                out,
                viewport.move_to(Pos { x: 0, y: 0 }),
                PrintStyledContent(local.with(LOCAL_COLOR))
            )?;
            if len <= viewport.width() {
                queue!(
                    out,
                    Print(" ".repeat((viewport.width() - len) as usize).as_str())
                )?;
            }

            let len = remote.width();
            queue!(
                out,
                viewport.move_to(Pos { x: 0, y: 1 }),
                PrintStyledContent(remote.with(REMOTE_COLOR))
            )?;
            if len <= viewport.width() {
                queue!(
                    out,
                    Print(" ".repeat((viewport.width() - len) as usize).as_str())
                )?;
            }

            let len = nodes.width() + sync.width() + conflicts.width() + sep.width() * 2;
            queue!(
                out,
                viewport.move_to(Pos { x: 0, y: 2 }),
                PrintStyledContent(nodes.with(NODE_COLOR)),
                Print(sep),
                PrintStyledContent(sync.with(SYNC_COLOR)),
                Print(sep),
                PrintStyledContent(conflicts.with(CONFLICT_COLOR)),
            )?;
            if len <= viewport.width() {
                queue!(
                    out,
                    Print(" ".repeat((viewport.width() - len) as usize).as_str())
                )?;
            }
        } else {
            let len = local.width()
                + remote.width()
                + nodes.width()
                + sync.width()
                + conflicts.width()
                + sep.width() * 4;

            queue!(
                out,
                viewport.move_to(Pos { x: 0, y: 0 }),
                PrintStyledContent(local.with(LOCAL_COLOR)),
                Print(sep),
                PrintStyledContent(remote.with(REMOTE_COLOR)),
                Print(sep),
                PrintStyledContent(nodes.with(NODE_COLOR)),
                Print(sep),
                PrintStyledContent(sync.with(SYNC_COLOR)),
                Print(sep),
                PrintStyledContent(conflicts.with(CONFLICT_COLOR)),
            )?;

            if len < viewport.width() {
                queue!(
                    out,
                    Print(
                        " ".repeat((viewport.width() - len as u16 - 12) as usize)
                            .as_str(),
                    )
                )?;
            }
        }
        Ok(())
    }
}
