use std::{fmt, io};

use crossterm::{
    event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    queue,
    style::{Color, Print, PrintStyledContent, Stylize},
    terminal,
};

use super::render::{Pos, Rect, Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    // Navigation
    Down,
    Up,
    Details,
    Enter,
    Back,
    Exit,
    // Operations
    Sync,
    SyncAll,
}

impl Action {
    fn desc(self) -> &'static str {
        match self {
            Action::Down => "down",
            Action::Up => "up",
            Action::Details => "details",
            Action::Enter => "enter",
            Action::Back => "go back",
            Action::Exit => "exit",
            Action::Sync => "sync.",
            Action::SyncAll => "sync. all",
        }
    }
}

pub struct KeyAction(&'static [KeyCode]);

impl KeyAction {
    pub fn matches(&self, event: &KeyEvent) -> bool {
        self.0.contains(&event.code)
    }

    fn desc(&self) -> KeyActionDesc {
        KeyActionDesc(self.0)
    }
}

struct KeyActionDesc(&'static [KeyCode]);

impl KeyActionDesc {
    fn key_code_str(code: KeyCode) -> &'static str {
        match code {
            KeyCode::Backspace => "⌫ ",
            KeyCode::Enter => "⏎ ",
            KeyCode::Up => "↑",
            KeyCode::Down => "↓",
            KeyCode::Esc => "esc",
            KeyCode::Char(' ') => "space",
            KeyCode::Char('j') => "j",
            KeyCode::Char('k') => "k",
            KeyCode::Char('q') => "q",
            KeyCode::Char('s') => "s",
            KeyCode::Char('S') => "S",
            _ => unreachable!(),
        }
    }
}

impl fmt::Display for KeyActionDesc {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (idx, code) in self.0.iter().enumerate() {
            write!(f, "{}", KeyActionDesc::key_code_str(*code))?;
            if idx < self.0.len() - 1 {
                write!(f, "/")?;
            }
        }
        Ok(())
    }
}

pub enum MenuItem {
    Action {
        action: Action,
        action_desc: String,
        key: KeyAction,
        key_desc: String,
    },
    Sep,
}

impl MenuItem {
    pub fn new_action(action: Action, key: KeyAction) -> MenuItem {
        let action_desc = action.desc().to_string();
        let key_desc = key.desc().to_string();
        MenuItem::Action {
            action,
            action_desc,
            key,
            key_desc,
        }
    }

    pub fn new_sep() -> MenuItem {
        MenuItem::Sep
    }

    pub fn key_desc_width(&self) -> u16 {
        match self {
            Self::Action { key_desc, .. } => key_desc.chars().count() as _,
            Self::Sep => 0,
        }
    }

    pub fn action_desc_width(&self) -> u16 {
        match self {
            Self::Action { action_desc, .. } => action_desc.chars().count() as _,
            Self::Sep => 0,
        }
    }

    const fn sep_str() -> &'static str {
        " "
    }
}

pub struct Menu {
    items: Vec<MenuItem>,
    disabled: Vec<Action>,
    max_key_width: u16,
    max_desc_width: u16,
}

impl Menu {
    pub fn new() -> Menu {
        let items = vec![
            MenuItem::new_action(
                Action::Down,
                KeyAction(&[KeyCode::Down, KeyCode::Char('j')]),
            ),
            MenuItem::new_action(Action::Up, KeyAction(&[KeyCode::Up, KeyCode::Char('k')])),
            MenuItem::new_action(Action::Enter, KeyAction(&[KeyCode::Enter])),
            MenuItem::new_action(Action::Back, KeyAction(&[KeyCode::Backspace])),
            MenuItem::new_action(Action::Details, KeyAction(&[KeyCode::Char(' ')])),
            MenuItem::new_sep(),
            MenuItem::new_action(Action::Sync, KeyAction(&[KeyCode::Char('s')])),
            MenuItem::new_action(Action::SyncAll, KeyAction(&[KeyCode::Char('S')])),
            MenuItem::new_sep(),
            MenuItem::new_action(Action::Exit, KeyAction(&[KeyCode::Esc, KeyCode::Char('q')])),
        ];
        let max_key_width = items.iter().map(|mi| mi.key_desc_width()).max().unwrap();
        let max_desc_width = items.iter().map(|mi| mi.action_desc_width()).max().unwrap();
        Menu {
            items,
            disabled: vec![],
            max_key_width,
            max_desc_width,
        }
    }

    pub fn enable(&mut self, action: Action, enabled: bool) {
        if enabled {
            // Remove the action from disabled
            self.disabled.retain(|&x| x != action);
        } else {
            // Add the action to disabled
            self.disabled.push(action);
        }
    }

    pub fn is_enabled(&self, action: Action) -> bool {
        !self.disabled.contains(&action)
    }

    pub fn action(&self, key_event: &KeyEvent) -> Option<Action> {
        if key_event.kind == KeyEventKind::Release {
            return None;
        }
        // emulate ctrl-c which doesn't work in raw mode
        if matches!(key_event.code, KeyCode::Char('c') | KeyCode::Char('C')) {
            if key_event.modifiers.contains(KeyModifiers::CONTROL) {
                return Some(Action::Exit);
            }
        }

        for mi in self.items.iter() {
            if let MenuItem::Action { action, key, .. } = mi {
                if key.matches(key_event) && self.is_enabled(*action) {
                    return Some(*action);
                }
            }
        }

        return None;
    }

    pub fn max_size(&self) -> Size {
        let width =
            self.max_key_width + self.max_desc_width + MenuItem::sep_str().chars().count() as u16;
        let height = self.items.len() as u16 + 1;
        Size { width, height }
    }

    pub fn render(&self, viewport: Rect, focus: bool) -> anyhow::Result<()> {
        let mut out = io::stdout();

        let sep = MenuItem::sep_str();
        let sep_count = sep.chars().count() as u16;

        let title = "FSYNC NAVIGATOR";
        let title_width = title.chars().count() as u16;

        let key_width = self.max_key_width;
        let desc_width = self.max_desc_width;
        let menu_width = key_width + desc_width + sep_count;
        let menu_width = menu_width.max(title_width);

        let title_start = menu_width / 2 - (title_width) / 2;
        let pos = viewport.abs_pos(Pos { x: 0, y: 0 });
        queue!(
            out,
            pos.move_to(),
            Print(" ".repeat(title_start as usize)),
            if focus {
                PrintStyledContent(title.cyan())
            } else {
                PrintStyledContent(title.with(Color::Grey).dim())
            },
            terminal::Clear(terminal::ClearType::UntilNewLine),
        )?;

        let mut y = 1;

        // Render each menu item
        for (idx, item) in self.items.iter().enumerate() {
            y = 1 + idx as u16;
            if y >= viewport.height() {
                break;
            }

            match item {
                MenuItem::Action {
                    action,
                    action_desc,
                    key_desc,
                    ..
                } => {
                    let enabled = self.is_enabled(*action) && focus;
                    let key_start =
                        menu_width - desc_width - sep_count - key_desc.chars().count() as u16;
                    let pos = viewport.abs_pos(Pos { x: 0, y });
                    queue!(
                        out,
                        pos.move_to(),
                        Print(" ".repeat(key_start as usize)),
                        if enabled {
                            PrintStyledContent(key_desc.as_str().cyan())
                        } else {
                            PrintStyledContent(key_desc.as_str().with(Color::Grey).dim())
                        },
                        PrintStyledContent(sep.grey().dim()),
                        PrintStyledContent(action_desc.as_str().grey()),
                        terminal::Clear(terminal::ClearType::UntilNewLine),
                    )?;
                }
                MenuItem::Sep => {
                    let pos = viewport.abs_pos(Pos { x: 0, y });
                    queue!(
                        out,
                        pos.move_to(),
                        terminal::Clear(terminal::ClearType::UntilNewLine)
                    )?;
                }
            }
        }

        // clear the rest until footer
        if y < viewport.height() - 1 {
            for y in y + 1..viewport.height() {
                let pos = viewport.abs_pos(Pos { x: 0, y });
                queue!(
                    out,
                    pos.move_to(),
                    terminal::Clear(terminal::ClearType::UntilNewLine),
                )?;
            }
        }

        Ok(())
    }
}

impl From<Action> for KeyAction {
    fn from(action: Action) -> Self {
        KeyAction(match action {
            Action::Down => &[KeyCode::Down, KeyCode::Char('j')],
            Action::Up => &[KeyCode::Up, KeyCode::Char('k')],
            Action::Details => &[KeyCode::Char(' ')],
            Action::Enter => &[KeyCode::Enter],
            Action::Back => &[KeyCode::Backspace],
            Action::Exit => &[KeyCode::Esc, KeyCode::Char('q')],
            Action::Sync => &[KeyCode::Char('s')],
            Action::SyncAll => &[KeyCode::Char('S')],
        })
    }
}
