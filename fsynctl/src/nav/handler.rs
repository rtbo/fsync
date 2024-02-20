use crossterm::event;
use fsync::path::Path;

use crate::nav::ctx;

use super::{menu::Action, render::Size};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandlerResult {
    Continue,
    Exit,
}

impl super::Navigator {
    pub async fn handle_event(&mut self, event: event::Event) -> anyhow::Result<HandlerResult> {
        use HandlerResult::*;

        // super::log_msg(&format!("{:?}", event));

        match event {
            event::Event::Key(key_event) => return Ok(self.handle_key_event(key_event).await?),
            event::Event::Resize(width, height) => {
                self.size = Size { width, height };
            }
            event::Event::FocusGained => self.focus = true,
            event::Event::FocusLost => self.focus = false,
            _ => {}
        }
        Ok(Continue)
    }
}

impl super::Navigator {
    async fn handle_key_event(&mut self, key_event: event::KeyEvent) -> anyhow::Result<HandlerResult> {
        use HandlerResult::*;

        let action = self.menu.action(&key_event);

        if let Some(action) = action {
            if self.menu.is_enabled(action) {
                return Ok(self.execute_action(action).await?);
            }
        }

        Ok(Continue)
    }

    async fn execute_action(&mut self, action: Action) -> anyhow::Result<HandlerResult> {
        use HandlerResult::*;

        let mut operated = false;

        match action {
            Action::Exit => return Ok(Exit),
            Action::Down => {
                self.cur_child = (self.cur_child + 1) % self.children.len();
                self.check_cur_child();
            }
            Action::Up => {
                if self.cur_child > 0 {
                    self.cur_child -= 1;
                } else {
                    self.cur_child = self.children.len() - 1;
                }
                self.check_cur_child();
            }
            Action::Details => {
                if self.detailed_child == Some(self.cur_child) {
                    self.detailed_child = None;
                } else {
                    self.detailed_child = Some(self.cur_child);
                }
            }
            Action::Enter => {
                self.open_cur_child().await?;
            }
            Action::Back => {
                self.open_parent().await?;
            }
            Action::Sync => {
                let child = self.cur_child_node();
                if let Some(child) = child {
                    let path = child.entry().path().to_owned();
                    if child.is_local_only() {
                        self.client.operate(ctx(), fsync::Operation::CopyLocalToRemote(path)).await.unwrap()?;
                        operated = true;
                    } else if child.is_remote_only() {
                        self.client.operate(ctx(), fsync::Operation::CopyRemoteToLocal(path)).await.unwrap()?;
                        operated = true;
                    }
                }
            }
            Action::SyncAll => {}
        }

        if operated {
            let path = self.node.entry().path().to_owned();
            let (node, children) = super::node_and_children(&self.client, &path).await?;
            self.node = node;
            self.children = children;
        }

        Ok(Continue)
    }

    async fn open_entry(&mut self, path: &Path) -> anyhow::Result<()> {
        let (node, children) = super::node_and_children(&self.client, &path).await?;
        self.node = node;
        self.children = children;
        self.check_cur_node();
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

    pub fn check_cur_child(&mut self) {
        let node = self.cur_child_node();

        if let Some(node) = node {
            let is_dir = node.entry().is_safe_dir();
            let is_not_sync = node.entry().is_local_only() || node.entry().is_remote_only();
            self.menu.enable(Action::Enter, is_dir);
            self.menu.enable(Action::Sync, is_not_sync);
            self.menu.enable(Action::SyncAll, is_not_sync && is_dir);
        }
    }

    pub fn check_cur_node(&mut self) {
        self.menu.enable(Action::Back, !self.node.path().is_root());
    }
}
