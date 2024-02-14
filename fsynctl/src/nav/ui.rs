use std::io::{self, Write};

use crossterm::{cursor::MoveTo, queue, style::{Color, Print, PrintStyledContent, Stylize}, terminal};
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

    fn print_command(&self) -> PrintStyledContent<char> {
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

impl super::Navigator {
    pub fn render(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();

        queue!(out, terminal::Clear(terminal::ClearType::All))?;

        self.render_menu()?;

        if self.node.entry().is_safe_dir() {
            self.render_dir((0, 0))?;
        } else {
            todo!()
        }

        self.render_footer()?;

        out.flush()?;
        Ok(())
    }
}

impl super::Navigator {
    fn render_menu(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();

        let sep = " : ";
        let sep_count = 3;

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

        let title = "FSYNC NAVIGATOR";
        queue!(
            out,
            MoveTo(self.size.0 - menu_width / 2 - (title.len() as u16) / 2, 0),
            PrintStyledContent(title.cyan()),
        )?;

        // Render each menu item
        for (idx, (action, Menu { key, desc })) in menu.into_iter().enumerate() {
            let enabled = self.is_enabled(action);
            let key_start = self.size.0 - desc_width - sep_count - key.chars().count() as u16;
            queue!(
                out,
                MoveTo(key_start, 1 + idx as u16),
                if enabled {
                    PrintStyledContent(key.cyan())
                } else {
                    PrintStyledContent(key.with(Color::Grey).dim())
                },
                PrintStyledContent(sep.grey().dim()),
                PrintStyledContent(desc.grey()),
            )?;
        }

        out.flush()?;
        Ok(())
    }

    fn render_dir(&self, orig: (u16, u16)) -> anyhow::Result<()> {
        let tag = Tag::from(self.node.entry());
        let node = &self.node;
        let mut out = io::stdout();

        queue!(
            out,
            MoveTo(orig.0, orig.1),
            tag.print_command(),
            Print(" "),
            Print(entry_print_path(node.entry())),
        )?;

        if self.node.children_have_conflicts() {
            queue!(
                out,
                PrintStyledContent(
                    format!(" [{}]", node.children_conflict_count()).with(Color::Red),
                )
            )?;
        }

        // for long paths, add spaces to ensure a space between the path and the title
        queue!(out, Print("  "))?;

        let mut pos = (orig.0, orig.1 + 1);

        for (idx, child) in self.children.iter().enumerate() {
            pos = self.render_child_node(pos, idx, child)?;
        }

        Ok(())
    }

    fn render_child_node(
        &self,
        pos: (u16, u16),
        idx: usize,
        child: &EntryNode,
    ) -> anyhow::Result<(u16, u16)> {
        let is_current = idx == self.cur_child;
        let _is_detailed = Some(idx) == self.detailed_child;
        let tag = Tag::from(child.entry());

        let mut out = io::stdout();

        queue!(out, MoveTo(pos.0, pos.1), tag.print_command(), Print(" "),)?;

        let path_col = if is_current {
            queue!(out, PrintStyledContent("> ".with(Color::Blue)))?;
            Color::Blue
        } else {
            queue!(out, Print("  "))?;
            Color::Reset
        };

        queue!(
            out,
            PrintStyledContent(entry_print_name(child.entry()).with(path_col))
        )?;

        if child.children_have_conflicts() {
            queue!(
                out,
                PrintStyledContent(
                    format!(" [{}]", child.children_conflict_count()).with(Color::Red),
                )
            )?;
        }

        // for long names, add spaces to ensure a space between the name and the title
        queue!(out, Print("  "))?;

        Ok((pos.0, pos.1 + 1))
    }

    // show the footer where each tag is explained
    fn render_footer(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();
        let tags = [Tag::sync(), Tag::local(), Tag::remote(), Tag::conflict()];
        let num_tags = tags.len() as u16;
        let pad = 1;
        let min_spacing = pad * (num_tags - 1);

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

        let can_print_long = width_long_desc < (self.size.0 - min_spacing) / num_tags;
        let can_print_short = width_short_desc < (self.size.0 - min_spacing) / num_tags;

        let mut pos = 0;
        for Tag {
            tag,
            color,
            desc,
            desc_short,
        } in tags
        {
            let mut dd = if can_print_long { desc } else { desc_short };

            let (start, end) = if can_print_long || can_print_short {
                (pos, pos + self.size.0 / num_tags)
            } else {
                (pos, pos + pad + 4 + desc_short.chars().count() as u16)
            };
            let width = end - start;

            // min is used here to avoid subtraction overflow
            let print_len = width.min(4 + dd.chars().count() as u16);
            let print_start = if can_print_long || can_print_short {
                start + width / 2 - print_len / 2
            } else {
                start
            };
            if end - dd.chars().count() as u16 > self.size.0 {
                break;
            }
            if end > self.size.0 {
                dd = &dd[0..dd.len() - (end - self.size.0) as usize];
            }

            queue!(
                out,
                MoveTo(print_start, self.size.1 - 1),
                PrintStyledContent(format!("{tag} : {dd}").with(color))
            )?;

            pos = end;
        }

        Ok(())
    }

}