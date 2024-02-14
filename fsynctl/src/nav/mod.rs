use std::{io, panic, sync::Arc, time::Duration};

use anyhow::Context;
use crossterm::{cursor, event, execute, terminal};
use fsync::{
    path::{Path, PathBuf},
    tree::EntryNode,
    FsyncClient,
};
use futures::{future, FutureExt};
use tarpc::context;

use crate::utils;

mod handler;
mod ui;

use handler::Action;

use self::handler::HandlerResult;

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
    // It was observed to receive initial Key enter event (shell prompt entry)
    // and resize event on Windows.
    if event::poll(Duration::from_millis(10))? {
        let _ = event::read();
        while event::poll(Duration::from_millis(0))? {
            let _ = event::read();
        }
    }

    let mut nav = Navigator::new(client, &path).await?;

    loop {
        nav.render()?;
        if event::poll(Duration::from_millis(500))? {
            match nav.handle_event(event::read()?).await? {
                HandlerResult::Exit => break,
                HandlerResult::Continue => (),
            }
        }
    }

    Ok(())
}

async fn node_and_children(
    client: &FsyncClient,
    path: &Path,
) -> anyhow::Result<(EntryNode, Vec<EntryNode>)> {
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
    Ok((node, children?))
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
        let (node, children) = node_and_children(&client, path).await?;

        let disabled_actions = vec![];

        Ok(Self {
            client,

            size: terminal::size()?,
            disabled_actions,

            node,
            children,
            cur_child: 0,
            detailed_child: None,
        })
    }
}
