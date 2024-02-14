use crossterm::event::{self, KeyCode, KeyEventKind, KeyModifiers};
use fsync::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Down,
    Up,
    Details,
    Enter,
    Back,
    Exit,
}

pub const ACTIONS: &[Action] = &[
    Action::Down,
    Action::Up,
    Action::Details,
    Action::Enter,
    Action::Back,
    Action::Exit,
];

pub enum HandlerResult {
    Continue,
    Exit,
}

impl super::Navigator {
    pub async fn handle_event(&mut self, event: event::Event) -> anyhow::Result<HandlerResult> {
        match event {
            event::Event::Resize(width, height) => {
                self.size = (width, height);
            }
            event::Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Char('q') | KeyCode::Esc => {
                    return Ok(HandlerResult::Exit);
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    // faking ctrl-c, which won't work in raw mode otherwise
                    return Ok(HandlerResult::Exit);
                }

                KeyCode::Down | KeyCode::Char('j') if self.is_enabled(Action::Down) => {
                    self.cur_child = (self.cur_child + 1) % self.children.len();
                    self.check_cur_child();
                }

                KeyCode::Up | KeyCode::Char('k') if self.is_enabled(Action::Up) => {
                    if self.cur_child > 0 {
                        self.cur_child -= 1;
                    } else {
                        self.cur_child = self.children.len() - 1;
                    }
                    self.check_cur_child();
                }

                KeyCode::Char(' ') if self.is_enabled(Action::Details) => {
                    if self.detailed_child == Some(self.cur_child) {
                        self.detailed_child = None;
                    } else {
                        self.detailed_child = Some(self.cur_child);
                    }
                }

                KeyCode::Enter if self.is_enabled(Action::Enter) => {
                    self.open_cur_child().await?;
                }
                KeyCode::Backspace if self.is_enabled(Action::Back) => {
                    self.open_parent().await?;
                }
                _ => {}
            },
            _ => {}
        }
        Ok(HandlerResult::Continue)
    }

    pub fn is_enabled(&self, action: Action) -> bool {
        !self.disabled_actions.contains(&action)
    }
}

impl super::Navigator {
    async fn open_entry(&mut self, path: &Path) -> anyhow::Result<()> {
        let (node, children) = super::node_and_children(&self.client, &path).await?;
        self.node = node;
        self.children = children;
        self.enable(Action::Back, !self.node.path().is_root());
        Ok(())
    }

    async fn open_parent(&mut self) -> anyhow::Result<()> {
        if self.node.path().is_root() {
            return Ok(());
        }
        let cur_name = self.node.name().unwrap().to_owned();
        let parent_path = self.node.path().parent().unwrap().to_owned();

        self.open_entry(&parent_path).await?;
        self.cur_child = self
            .children
            .iter()
            .position(|n| n.name().unwrap() == &cur_name)
            .unwrap();
        self.check_cur_child();
        
        Ok(())
    }

    async fn open_cur_child(&mut self) -> anyhow::Result<()> {
        let path = self.children[self.cur_child].path().to_owned();

        self.open_entry(&path).await?;
        self.cur_child = 0;
        self.check_cur_child();

        Ok(())
    }

    fn check_cur_child(&mut self) {
        let child = &self.children[self.cur_child];
        self.enable(Action::Enter, child.entry().is_safe_dir());
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
}
