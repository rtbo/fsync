//! Rendering module for the navigator.
//! For each rendered frame, each character of the screen buffer is written
//! once and only once. This is done without clearing the screen first 
//! to avoid flickering.
use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal,
};
use fsync::tree::{Entry, EntryNode};

use super::handler::{Action, ACTIONS};

pub struct Menu {
    pub key: &'static str,
    pub desc: &'static str,
}

impl From<Action> for Menu {
    fn from(action: Action) -> Self {
        match action {
            Action::Down => Menu {
                key: "↓/j",
                desc: "down",
            },
            Action::Up => Menu {
                key: "↑/k",
                desc: "up",
            },
            Action::Details => Menu {
                key: "space",
                desc: "details",
            },
            Action::Enter => Menu {
                key: "enter",
                desc: "select",
            },
            Action::Back => Menu {
                key: "backspace",
                desc: "go back",
            },
            Action::Exit => Menu {
                key: "esc/q",
                desc: "exit",
            },
        }
    }
}

fn entry_print_path(entry: &Entry) -> String {
    let path = entry.path().to_string();
    if entry.is_safe_dir() && !entry.path().is_root() {
        format!("{path}/")
    } else {
        path
    }
}

fn entry_print_name(entry: &Entry) -> String {
    let name = entry.path().file_name().unwrap_or_default().to_string();
    if entry.is_safe_dir() {
        format!("{name}/")
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
            color: Color::Green,
            tag: 'S',
            desc: "Synchronized",
            desc_short: "Sync",
        }
    }

    // local only tag
    const fn local() -> Self {
        Tag {
            color: Color::Reset,
            tag: 'L',
            desc: "Local only",
            desc_short: "Local",
        }
    }

    // remote only tag
    const fn remote() -> Self {
        Tag {
            color: Color::Cyan,
            tag: 'R',
            desc: "Remote only",
            desc_short: "Remote",
        }
    }

    // conflict tag
    const fn conflict() -> Self {
        Tag {
            color: Color::Red,
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
    fn move_to(&self) -> MoveTo {
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
    fn width(&self) -> u16 {
        self.size.width
    }

    fn height(&self) -> u16 {
        self.size.height
    }

    fn right(&self) -> u16 {
        self.top_left.x + self.width()
    }

    fn abs_pos(&self, pos: Pos) -> Pos {
        Pos {
            x: self.top_left.x + pos.x,
            y: self.top_left.y + pos.y,
        }
    }

    fn crop_right(&self, w: u16) -> Rect {
        Rect {
            top_left: self.top_left,
            size: Size {
                width: self.width() - w,
                height: self.height(),
            },
        }
    }

    fn crop_top(&self, h: u16) -> Rect {
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

impl super::Navigator {
    pub fn render(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();

        let height = self.size.height - 1;

        let menu_width = self.render_menu(height)?;

        let viewport = Rect {
            top_left: Pos { x: 0, y: 0 },
            size: Size {
                width: self.size.width - menu_width,
                height,
            },
        };

        if self.node.entry().is_safe_dir() {
            self.render_dir(&viewport)?;
        } else {
            todo!()
        }

        self.render_footer()?;

        out.flush()?;
        Ok(())
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

impl super::Navigator {
    // renders the menu on the right, and clear the rest
    // of the right side over given height
    fn render_menu(&self, height: u16) -> anyhow::Result<u16> {
        let mut out = io::stdout();

        let sep = " : ";
        let sep_count = 3;

        let title = "FSYNC NAVIGATOR";

        let menu: Vec<(Action, Menu)> = ACTIONS.iter().map(|a| (*a, Menu::from(*a))).collect();

        // compute key width
        let key_width = menu
            .iter()
            .map(|(_, m)| m.key.chars().count() as u16)
            .max()
            .unwrap_or(0);
        // compute desc width
        let desc_width = menu
            .iter()
            .map(|(_, m)| m.desc.chars().count() as u16)
            .max()
            .unwrap_or(0);
        let menu_width = key_width + desc_width + sep_count;
        let menu_width = menu_width.max(title.chars().count() as u16);

        let vp = Rect {
            top_left: Pos {
                x: self.size.width - menu_width,
                y: 0,
            },
            size: Size {
                width: menu_width,
                height,
            },
        };

        let title_start = menu_width / 2 - (title.len() as u16) / 2;
        let pos = vp.abs_pos(Pos { x: 0, y: 0 });
        queue!(
            out,
            pos.move_to(),
            Print(" ".repeat(title_start as usize)),
            if self.focus {
                PrintStyledContent(title.cyan())
            } else {
                PrintStyledContent(title.with(Color::Grey).dim())
            },
            terminal::Clear(terminal::ClearType::UntilNewLine),
        )?;

        let mut y = 1;

        // Render each menu item
        for (idx, (action, Menu { key, desc })) in menu.into_iter().enumerate() {
            y = 1 + idx as u16;
            if y >= height {
                break;
            }

            let enabled = self.is_enabled(action) && self.focus;
            let key_start = menu_width - desc_width - sep_count - key.chars().count() as u16;
            let pos = vp.abs_pos(Pos { x: 0, y });
            queue!(
                out,
                pos.move_to(),
                Print(" ".repeat(key_start as usize)),
                if enabled {
                    PrintStyledContent(key.cyan())
                } else {
                    PrintStyledContent(key.with(Color::Grey).dim())
                },
                PrintStyledContent(sep.grey().dim()),
                PrintStyledContent(desc.grey()),
                terminal::Clear(terminal::ClearType::UntilNewLine),
            )?;
        }

        // clear the rest until footer
        if y < height - 1 {
            for y in y + 1..height {
                let pos = vp.abs_pos(Pos { x: 0, y });
                queue!(
                    out,
                    pos.move_to(),
                    terminal::Clear(terminal::ClearType::UntilNewLine),
                )?;
            }
        }

        Ok(menu_width)
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

    fn render_dir(&self, viewport: &Rect) -> anyhow::Result<()> {
        let tag = Tag::from(self.node.entry());
        let node = &self.node;

        let mut out = io::stdout();

        let path = entry_print_path(node.entry());
        let pos = viewport.abs_pos(Pos { x: 0, y: 0 });
        queue!(out, pos.move_to(), tag.print(), Print(" "), Print(&path),)?;

        let mut w = path.len() as u16 + 2;

        if self.node.children_have_conflicts() {
            let cf = format!(" [{}]", node.children_conflict_count());
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
            pos.y += self.render_child_node(idx, child, pos, children_vp, scroll_offset)?;
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
            3
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
    ) -> anyhow::Result<u16> {
        let height = self.compute_child_height(idx);
        let start_y = pos.y as i16 + scroll_offset;
        let end_y = start_y + height as i16 - 1;

        // start_y is the line showing the child name
        // end_y is the last line of details

        if end_y < 0 {
            return Ok(height);
        }

        let mut out = io::stdout();

        let is_current = idx == self.cur_child;
        let is_detailed = Some(idx) == self.detailed_child;

        let mut w = 0;

        if start_y >= 0 && start_y < viewport.height() as i16 {
            let tag = Tag::from(child.entry());
            let abs_pos = viewport.abs_pos(Pos {
                x: pos.x,
                y: start_y as u16,
            });
            queue!(out, abs_pos.move_to(), tag.print(), Print(" "))?;
            w += 2;

            let path_col = if is_current {
                let sign = if is_detailed { "v " } else { "> " };
                queue!(out, PrintStyledContent(sign.with(Color::Blue)))?;
                Color::Blue
            } else {
                queue!(out, Print("  "))?;
                Color::Reset
            };
            w += 2;

            let name = entry_print_name(child.entry());
            queue!(out, PrintStyledContent(name.as_str().with(path_col)))?;
            w += name.len() as u16;

            if child.children_have_conflicts() {
                let cf = format!(" [{}]", child.children_conflict_count());
                queue!(out, PrintStyledContent(cf.as_str().with(Color::Red),))?;
                w += cf.len() as u16;
            }

            // for long names, add spaces to ensure a space between the name and the title
            queue!(out, Print("  "))?;
            w += 2;

            if w < viewport.width() {
                queue!(
                    out,
                    Print(" ".repeat((viewport.width() - w) as usize).as_str(),)
                )?;
            }
        }

        if is_detailed {
            self.render_child_details(idx, child, pos.x, start_y + 1, viewport)?;
        }

        Ok(height)
    }

    fn render_child_details(
        &self,
        _idx: usize,
        _child: &EntryNode,
        x: u16,
        y: i16,
        viewport: Rect,
    ) -> anyhow::Result<u16> {
        let mut out = io::stdout();

        // line 1
        if y >= 0 && y < viewport.height() as i16 {
            let pos = viewport.abs_pos(Pos { x, y: y as u16 });
            queue!(
                out,
                pos.move_to(),
                Print(" ".repeat((viewport.width() - x) as usize).as_str()),
            )?;
        }
        // line 2
        if (y + 1) >= 0 && (y + 1) < viewport.height() as i16 {
            let pos = viewport.abs_pos(Pos {
                x,
                y: (y + 1) as u16,
            });
            queue!(
                out,
                pos.move_to(),
                Print(" ".repeat((viewport.width() - x) as usize).as_str()),
            )?;
        }

        Ok(2)
    }

    // show the footer where each tag is explained
    fn render_footer(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();
        let tags = [Tag::sync(), Tag::local(), Tag::remote(), Tag::conflict()];
        let num_tags = tags.len() as u16;
        let pad = 1;
        let min_spacing = pad * (num_tags - 1);

        queue!(out, MoveTo(0, self.size.height - 1))?;

        // 3 print modes depending on available width:
        //  - long desc
        //      width is divided in equal parts and printed in each tag long description
        //  - short desc
        //      width is divided in equal parts and printed in each tag short description
        //  - compact
        //      print each tag in a compact manner with short desc

        let width_long_desc: u16 = tags
            .iter()
            .map(|t| 4 + t.desc.chars().count() as u16)
            .max()
            .unwrap_or(0);

        let width_short_desc: u16 = tags
            .iter()
            .map(|t| 4 + t.desc_short.chars().count() as u16)
            .max()
            .unwrap_or(0);

        let can_print_long = width_long_desc < (self.size.width - min_spacing) / num_tags;
        let can_print_short = width_short_desc < (self.size.width - min_spacing) / num_tags;

        if !can_print_long && !can_print_short {
            for Tag {
                tag,
                color,
                desc_short,
                ..
            } in tags
            {
                queue!(
                    out,
                    PrintStyledContent(format!("{tag} : {desc_short}").with(color)),
                )?;
            }
            queue!(out, terminal::Clear(terminal::ClearType::UntilNewLine))?;
            return Ok(());
        }

        let width = self.size.width / num_tags;
        for Tag {
            tag,
            color,
            desc,
            desc_short,
        } in tags
        {
            let dd = if can_print_long { desc } else { desc_short };

            let print_len = 4 + dd.chars().count() as u16;
            let print_start = width / 2 - print_len / 2;

            queue!(
                out,
                Print(" ".repeat(print_start as usize).as_str()),
                PrintStyledContent(format!("{tag} : {dd}").with(color)),
                Print(
                    " ".repeat((width - print_start - print_len) as usize)
                        .as_str()
                ),
            )?;
        }

        Ok(())
    }
}
