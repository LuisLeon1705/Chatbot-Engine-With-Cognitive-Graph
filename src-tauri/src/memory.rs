//! Memory manager: view and edit the graphs (world and characters) by hand.
//! Every edit is saved to the `.mem` right away.

use std::collections::HashMap;

use i18n::tr;

use memory_core::{EdgeDetail, EdgeKind, GraphView, MemoryHandler, MemoryNode, NodeId, NodeKind, NodePatch, STORY_OPEN, Story, StoryTime, unix_now};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::state::{AppState, Res, lang};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeDto {
    id: NodeId,
    kind: NodeKind,
    kind_code: u8,
    label: String,
    content: String,
    weight: f32,
    valence: f32,
    created_at: i64,
    last_accessed: i64,
    access_count: u32,
    degree: usize,
    is_root: bool,
    /// Whether the node stands for a character with a sheet of their own.
    linked: bool,
    /// Story time from which it exists.
    since: Story,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GraphDto {
    name: String,
    scope: memory_core::Scope,
    view: GraphView,
    nodes: Vec<NodeDto>,
}

fn handler(state: &AppState, world: &str, graph: &str) -> Res<MemoryHandler> {
    let place = state.place(world)?;
    state.place_graph(&place, graph)
}

fn save(h: &MemoryHandler) -> Res<()> {
    h.save().map_err(|e| e.to_string())
}

/// Full graph: view for drawing + list of nodes with their content.
#[tauri::command]
pub fn memory_graph(state: State<'_, AppState>, world: String, graph: String) -> Res<GraphDto> {
    let h = handler(&state, &world, &graph)?;
    let view = h.view();
    Ok(h.read(|g| GraphDto {
        name: g.meta.name.clone(),
        scope: g.meta.scope,
        view,
        nodes: g
            .nodes()
            .map(|(id, n)| NodeDto {
                id,
                kind: n.kind,
                kind_code: n.kind as u8,
                label: n.label.clone(),
                content: n.content.clone(),
                weight: n.weight,
                valence: n.valence,
                created_at: n.created_at,
                last_accessed: n.last_accessed,
                access_count: n.access_count,
                degree: g.degree(id),
                is_root: n.kind.is_root(),
                linked: g.external(id).is_some(),
                since: n.since.into(),
            })
            .collect(),
    }))
}

#[tauri::command]
pub fn memory_edges(state: State<'_, AppState>, world: String, graph: String, node: NodeId) -> Res<Vec<EdgeDetail>> {
    let h = handler(&state, &world, &graph)?;
    Ok(h.read(|g| g.node_edges(node)))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NodeEdit {
    kind: Option<NodeKind>,
    label: Option<String>,
    content: Option<String>,
    weight: Option<f32>,
    valence: Option<f32>,
    since: Option<Story>,
}

#[tauri::command]
pub fn memory_update_node(state: State<'_, AppState>, world: String, graph: String, node: NodeId, edit: NodeEdit) -> Res<()> {
    let h = handler(&state, &world, &graph)?;
    h.write(|g| {
        let is_root = g.node(node).is_some_and(|n| n.kind.is_root());
        if let Some(since) = edit.since.filter(|_| !is_root) {
            g.set_since(node, since.into())?;
        }
        g.update_node(
            node,
            NodePatch {
                // The root keeps its kind; the rest can change kind freely.
                kind: edit.kind.filter(|_| !is_root),
                label: edit.label.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()),
                content: edit.content.map(|s| s.trim().to_owned()),
                weight: edit.weight,
                valence: edit.valence,
                ..Default::default()
            },
        )
    })
    .map_err(|e| e.to_string())?;
    save(&h)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewNode {
    kind: NodeKind,
    label: String,
    content: String,
    weight: f32,
    valence: f32,
    /// Connect the new node from this one (e.g. the root or a memory).
    link_from: Option<NodeId>,
    edge_kind: Option<EdgeKind>,
}

#[tauri::command]
pub fn memory_create_node(state: State<'_, AppState>, world: String, graph: String, node: NewNode) -> Res<NodeId> {
    if node.label.trim().is_empty() {
        return Err(tr!(lang(), "app.node_needs_name"));
    }
    let h = handler(&state, &world, &graph)?;
    let now = unix_now();
    let id = h
        .write(|g| -> Result<NodeId, memory_core::GraphError> {
            let id = g.add_node(
                MemoryNode::new(node.kind, node.label.trim(), node.content.trim(), now).with_weight(node.weight).with_valence(node.valence),
            )?;
            if let Some(from) = node.link_from {
                let kind = node.edge_kind.unwrap_or(if g.hangs_from_root(node.kind) { EdgeKind::Composes } else { EdgeKind::RelatedTo });
                g.set_edge(from, id, kind, node.weight.max(0.3), now)?;
            }
            Ok(id)
        })
        .map_err(|e| e.to_string())?;
    save(&h)?;
    Ok(id)
}

#[tauri::command]
pub fn memory_delete_node(state: State<'_, AppState>, world: String, graph: String, node: NodeId) -> Res<()> {
    let h = handler(&state, &world, &graph)?;
    h.write(|g| g.remove_node(node)).map_err(|e| e.to_string())?;
    save(&h)
}

#[tauri::command]
pub fn memory_set_edge(
    state: State<'_, AppState>,
    world: String,
    graph: String,
    from: NodeId,
    to: NodeId,
    kind: EdgeKind,
    weight: f32,
) -> Res<()> {
    let h = handler(&state, &world, &graph)?;
    h.write(|g| g.set_edge(from, to, kind, weight, unix_now())).map_err(|e| e.to_string())?;
    save(&h)
}

/// Deletes a link: the open one, or the closed one that ends at `until`.
#[tauri::command]
pub fn memory_delete_edge(state: State<'_, AppState>, world: String, graph: String, from: NodeId, to: NodeId, until: Option<Story>) -> Res<()> {
    let h = handler(&state, &world, &graph)?;
    let until: StoryTime = until.map_or(STORY_OPEN, Into::into);
    let removed = h.write(|g| g.disconnect_at(from, to, until) || g.disconnect_at(to, from, until));
    if !removed {
        return Err(tr!(lang(), "app.edge_missing"));
    }
    save(&h)
}

/// When a link holds: `until` picks the link (the open one is `"open"`); `valid_from` and
/// `valid_until` are its new ends. Closing it keeps it as the past.
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn memory_set_edge_validity(
    state: State<'_, AppState>,
    world: String,
    graph: String,
    from: NodeId,
    to: NodeId,
    until: Story,
    valid_from: Story,
    valid_until: Story,
) -> Res<()> {
    let h = handler(&state, &world, &graph)?;
    h.write(|g| g.set_edge_validity(from, to, until.into(), valid_from.into(), valid_until.into())).map_err(|e| e.to_string())?;
    save(&h)
}

/// Previews what the graph would recall for a text (without rehearsing or saving anything).
#[tauri::command]
pub fn memory_probe(state: State<'_, AppState>, world: String, graph: String, text: String) -> Res<HashMap<NodeId, f32>> {
    let h = handler(&state, &world, &graph)?;
    let p = h.config().activation.clone();
    Ok(h.read(|g| {
        let seeds = g.seeds(&text);
        g.spread(&seeds, &p).into_iter().filter(|(_, a)| *a > 0.0).collect()
    }))
}
