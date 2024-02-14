use std::{
    io::{self, Write},
    panic,
    sync::Arc,
    time::Duration,
};

use anyhow::Context;
use crossterm::{
    cursor::{self, MoveTo},
    event::{self, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal,
};
use fsync::{
    path::{Path, PathBuf},
    tree::{Entry, EntryNode},
    FsyncClient,
};
use futures::{future, FutureExt};
use tarpc::context;

use crate::utils;

#[derive(clap::Args, Debug)]
pub struct Args {
    /// Name of the fsyncd instance
    #[clap(long, short = 'n')]
    instance_name: Option<String>,

    /// A path to navigate to (defaults to '/')
    path: Option<PathBuf>,
}

fn ctx() -> context::Context {
    context::current()
}

pub async fn main(args: Args) -> anyhow::Result<()> {
    let instance_name = match &args.instance_name {
        Some(name) => name.clone(),
        None => {
            let name = utils::single_instance_name()?;
            if let Some(name) = name {
                name
            } else {
                anyhow::bail!("Could not find a single share, please specify --share-name command line argument");
            }
        }
    };

    let path = args.path.unwrap_or_else(PathBuf::root);
    let client = utils::instance_client(&instance_name).await?;

    let mut out = io::stdout();

    // to ensure correct terminal config clean-up, we need to catch errors
    // and panic

    execute!(out, terminal::EnterAlternateScreen, cursor::Hide)?;
    terminal::enable_raw_mode().expect("Should enable raw mode");

    let res = panic::AssertUnwindSafe(navigate(client, path))
        .catch_unwind()
        .await;

    terminal::disable_raw_mode().expect("Should able raw mode");
    execute!(out, cursor::Show, terminal::LeaveAlternateScreen)?;

    if let Err(err) = res {
        let desc = err.downcast_ref::<String>();
        if let Some(desc) = desc {
            eprintln!("Panic occured: {desc}");
        }
        panic::resume_unwind(err);
    } else {
        res.unwrap()
    }
}

async fn navigate(client: Arc<FsyncClient>, path: PathBuf) -> anyhow::Result<()> {
    // it is possible to receive start-up events, so we need to clear them.
    // Observation is to receive initial Key enter event (shell prompt entry)
    // and resize event on Windows.
    if event::poll(Duration::from_millis(10))? {
        let _ = event::read();
        while event::poll(Duration::from_millis(0))? {
            let _ = event::read();
        }
    }

    let mut navigator = Some(Navigator::new(client, &path).await?);
    while let Some(nav) = navigator {
        navigator = nav.navigate().await?;
    }
    Ok(())
}

struct Navigator {
    client: Arc<FsyncClient>,
    size: (u16, u16),
    disabled_actions: Vec<Action>,

    node: EntryNode,
    children: Vec<EntryNode>,
    cur_child: usize,
    detailed_child: Option<usize>,
}

impl Navigator {
    async fn new(client: Arc<FsyncClient>, path: &Path) -> anyhow::Result<Self> {
        let node = client
            .entry_node(ctx(), path.to_owned())
            .await
            .unwrap()?
            .with_context(|| format!("No entry found at {path}"))?;
        let child_futs = node.children().iter().map(|name| {
            let child_path = path.join(name);
            client.entry_node(ctx(), child_path)
        });
        let children: Result<Vec<_>, _> = future::try_join_all(child_futs)
            .await?
            .into_iter()
            .map(|c| c.map(|c| c.expect("No entry found at child")))
            .collect();

        let mut disabled_actions = vec![];
        if path.is_root() {
            disabled_actions.push(Action::Back);
        }

        Ok(Self {
            client,
            size: terminal::size()?,
            disabled_actions,

            node,
            children: children?,
            cur_child: 0,
            detailed_child: None,
        })
    }

    // navigate in the current entry, and return a new navigator
    // if user select a child entry, or to go back to parent
    // returns none to exit
    async fn navigate(mut self) -> anyhow::Result<Option<Navigator>> {
        self.render()?;

        loop {
            if event::poll(Duration::from_millis(500))? {
                let event = event::read()?;
                match event {
                    event::Event::Resize(width, height) => {
                        self.size = (width, height);
                        self.render()?;
                    }
                    event::Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            return Ok(None);
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            // faking ctrl-c, which won't work in raw mode otherwise
                            return Ok(None);
                        }

                        KeyCode::Down | KeyCode::Char('j') if self.is_enabled(Action::Down) => {
                            self.cur_child = (self.cur_child + 1) % self.children.len();
                            self.check_cur_child();
                            self.render()?;
                        }

                        KeyCode::Up | KeyCode::Char('k') if self.is_enabled(Action::Up) => {
                            if self.cur_child > 0 {
                                self.cur_child -= 1;
                            } else {
                                self.cur_child = self.children.len() - 1;
                            }
                            self.check_cur_child();
                            self.render()?;
                        }

                        KeyCode::Char(' ') if self.is_enabled(Action::Details) => {
                            if self.detailed_child == Some(self.cur_child) {
                                self.detailed_child = None;
                            } else {
                                self.detailed_child = Some(self.cur_child);
                            }
                            self.render()?;
                        }

                        KeyCode::Enter if self.is_enabled(Action::Enter) => {
                            let child = &self.children[self.cur_child];
                            return Ok(Some(Navigator::new(self.client, child.path()).await?));
                        }
                        KeyCode::Backspace if self.is_enabled(Action::Back) => {
                            if !self.node.path().is_root() {
                                let parent_path = self.node.path().parent().unwrap();
                                return Ok(Some(Navigator::new(self.client, parent_path).await?));
                            }
                        }
                        _ => {}
                    },
                    _ => {}
                }
            }
        }
    }

    fn check_cur_child(&mut self) {
        let child = &self.children[self.cur_child];
        self.enable(Action::Enter, child.entry().is_safe_dir());
    }

    fn is_enabled(&self, action: Action) -> bool {
        !self.disabled_actions.contains(&action)
    }

    fn enable(&mut self, action: Action, enabled: bool) {
        if enabled {
            // Remove the action from disabled_actions
            self.disabled_actions.retain(|&x| x != action);
        } else {
            // Add the action to disabled_actions
            self.disabled_actions.push(action);
        }
    }

    fn render(&self) -> anyhow::Result<()> {
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

    fn render_menu(&self) -> anyhow::Result<()> {
        let mut out = io::stdout();

        let sep = " : ";
        let sep_count = 3;

        let menu: Vec<(Action, Menu)> = ACTIONS.iter().map(|a| (*a, a.menu())).collect();
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

struct Menu {
    key: &'static str,
    desc: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Down,
    Up,
    Details,
    Enter,
    Back,
    Exit,
}

impl Action {
    fn menu(&self) -> Menu {
        match self {
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

const ACTIONS: &[Action] = &[
    Action::Down,
    Action::Up,
    Action::Details,
    Action::Enter,
    Action::Back,
    Action::Exit,
];

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
