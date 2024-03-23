use anyhow::Context;
use fsync::{path::Path, tree::EntryNode, FsyncClient};
use futures::future;
use tarpc::context;

pub fn ctx() -> context::Context {
    context::current()
}

pub async fn node_and_children(
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
