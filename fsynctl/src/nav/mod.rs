use std::{io, panic, sync::Arc, time::Duration};

use anyhow::Context;
use crossterm::{
    cursor,
    event::{self, EventStream},
    execute, terminal,
};
use fsync::{
    path::{Path, PathBuf},
    tree::EntryNode,
    FsyncClient,
};
use futures::{future, FutureExt, StreamExt};
use tarpc::context;
use tokio::time;

use crate::utils;

mod handler;
mod menu;
mod render;

use handler::HandlerResult;
use menu::Menu;
use render::Size;

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

    execute!(
        out,
        terminal::EnterAlternateScreen,
        cursor::Hide,
        event::EnableFocusChange
    )?;
    terminal::enable_raw_mode().expect("Should enable raw mode");

    let res = panic::AssertUnwindSafe(navigate(client, path))
        .catch_unwind()
        .await;

    terminal::disable_raw_mode().expect("Should able raw mode");
    execute!(
        out,
        event::DisableFocusChange,
        cursor::Show,
        terminal::LeaveAlternateScreen
    )?;

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
    use HandlerResult::*;

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
    let mut render_state = render::State::default();
    let mut reader = EventStream::new();
    let mut last_frame = time::Instant::now();
    let frame_dur = Duration::from_micros(((1.0 / render::ANIM_TPS as f64) * 1_000_000.0) as u64);

    loop {
        let animate = nav.render(&mut render_state).await?;

        let event = reader.next();

        let res = if animate {
            let elapsed = time::Instant::now() - last_frame;
            let delay = time::sleep(frame_dur - elapsed);

            tokio::select! {
                _ = delay => Continue,
                maybe_event = event => {
                    match maybe_event {
                        Some(Ok(event)) => nav.handle_event(event).await?,
                        _ => Continue,
                    }
                }
            }
        } else {
            match event.await {
                Some(Ok(event)) => nav.handle_event(event).await?,
                _ => Continue,
            }
        };

        if res == Exit {
            break;
        }

        last_frame = time::Instant::now();
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

// use std::{fs::File, io::Write, sync::Mutex};

// static LOG_FILE: Mutex<Option<File>> = Mutex::new(None);

// /// Logs a message to the "nav.log" file.
// /// This function is only for debugging purposes,
// /// because the raw terminal mode makes it difficult to
// /// debug otherwise.
// fn log_msg(message: &str) {
//     let mut log_file = LOG_FILE.lock().unwrap();
//     if log_file.is_none() {
//         *log_file = Some(File::create("nav.log").unwrap());
//     }
//     if let Some(file) = log_file.as_mut() {
//         writeln!(file, "{}", message).unwrap();
//     }
// }

struct Navigator {
    client: Arc<FsyncClient>,

    size: Size,
    focus: bool,
    menu: Menu,

    node: EntryNode,
    children: Vec<EntryNode>,
    cur_child: usize,
    detailed_child: Option<usize>,
}

impl Navigator {
    async fn new(client: Arc<FsyncClient>, path: &Path) -> anyhow::Result<Self> {
        let (node, children) = node_and_children(&client, path).await?;

        let mut nav = Self {
            client,

            size: terminal::size()?.into(),
            focus: true,
            menu: Menu::new(),

            node,
            children,
            cur_child: 0,
            detailed_child: None,
        };
        nav.check_cur_node();
        nav.check_cur_child();
        Ok(nav)
    }

    fn cur_child_node(&self) -> Option<&EntryNode> {
        self.children.get(self.cur_child)
    }
}
