//! Deterministic presentation graph layout and obstacle-aware orthogonal routing.
//! Pairwise adjacency guides placement only; rendered nets retain all terminals.
use crate::{Layout, NetRoute};
use sim_inspect::{DiagramState, Point, PortKind, SystemDescription};
use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};

pub const WIDTH: f32 = 288.;
pub const HEADER: f32 = 88.;
pub const ROW: f32 = 24.;
const GRID: f32 = 16.;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}
impl Side {
    pub fn sign(self) -> f32 {
        if self == Self::Left { -1. } else { 1. }
    }
}
#[derive(Debug, Clone, Copy)]
pub struct NodeBox {
    pub position: Point,
    pub height: f32,
}
impl NodeBox {
    pub fn contains(&self, p: Point, margin: f32) -> bool {
        p.x > self.position.x - margin
            && p.x < self.position.x + WIDTH + margin
            && p.y > self.position.y - margin
            && p.y < self.position.y + self.height + margin
    }
}
fn leaves(d: &SystemDescription) -> Vec<&sim_inspect::PortDescription> {
    let parents: BTreeSet<_> = d
        .ports
        .values()
        .filter_map(|p| p.composite_parent.as_ref())
        .collect();
    d.ports
        .values()
        .filter(|p| !parents.contains(&p.id))
        .collect()
}
fn adjacency(
    d: &SystemDescription,
    cancelled: &AtomicBool,
) -> Option<BTreeMap<String, BTreeSet<String>>> {
    let mut graph: BTreeMap<_, BTreeSet<_>> = d
        .components
        .keys()
        .map(|id| (id.clone(), BTreeSet::new()))
        .collect();
    for net in d.nets.values() {
        let nodes: BTreeSet<_> = net
            .ports
            .iter()
            .filter_map(|id| d.ports.get(id))
            .map(|p| &p.component)
            .collect();
        for a in &nodes {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            for b in &nodes {
                if a != b {
                    graph.get_mut(*a).unwrap().insert((*b).clone());
                }
            }
        }
    }
    Some(graph)
}
/// Signal-source roots, breadth layers, then barycentric ordering reduce wire
/// length while keeping disconnected regions separate. IDs resolve every tie.
pub fn initial_state(d: &SystemDescription) -> DiagramState {
    initial_state_cancellable(d, &AtomicBool::new(false)).expect("not cancelled")
}
pub fn initial_state_cancellable(
    d: &SystemDescription,
    cancelled: &AtomicBool,
) -> Option<DiagramState> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut state = DiagramState::new(d);
    let graph = adjacency(d, cancelled)?;
    let mut port_counts = BTreeMap::new();
    for p in leaves(d) {
        *port_counts.entry(p.component.clone()).or_insert(0usize) += 1;
    }
    let mut directed: BTreeMap<String, Vec<(String, bool)>> = BTreeMap::new();
    for net in d.nets.values() {
        let outputs: Vec<_> = net
            .ports
            .iter()
            .filter_map(|id| d.ports.get(id))
            .filter(|p| matches!(p.schema, PortKind::SignalOutput { .. }))
            .collect();
        let inputs: Vec<_> = net
            .ports
            .iter()
            .filter_map(|id| d.ports.get(id))
            .filter(|p| matches!(p.schema, PortKind::SignalInput { .. }))
            .collect();
        for a in &outputs {
            for b in &inputs {
                if a.component != b.component {
                    directed
                        .entry(a.component.clone())
                        .or_default()
                        .push((b.component.clone(), true));
                    directed
                        .entry(b.component.clone())
                        .or_default()
                        .push((a.component.clone(), false));
                }
            }
        }
    }
    let mut incoming = BTreeSet::new();
    let mut outgoing = BTreeSet::new();
    for p in d.ports.values() {
        match p.schema {
            PortKind::SignalInput { .. } => {
                incoming.insert(p.component.clone());
            }
            PortKind::SignalOutput { .. } => {
                outgoing.insert(p.component.clone());
            }
            _ => {}
        }
    }
    let mut remaining: BTreeSet<_> = graph.keys().cloned().collect();
    let mut island_top = 0.;
    while !remaining.is_empty() {
        let root = remaining
            .iter()
            .min_by_key(|id| {
                (
                    !outgoing.contains(*id) || incoming.contains(*id),
                    Reverse(graph[*id].len()),
                    (*id).clone(),
                )
            })
            .unwrap()
            .clone();
        let mut queue = VecDeque::from([(root.clone(), 0usize)]);
        remaining.remove(&root);
        let mut layers: Vec<Vec<String>> = vec![];
        while let Some((node, depth)) = queue.pop_front() {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            if layers.len() <= depth {
                layers.push(vec![]);
            }
            layers[depth].push(node.clone());
            for neighbor in &graph[&node] {
                if remaining.remove(neighbor) {
                    queue.push_back((neighbor.clone(), depth + 1));
                }
            }
        }
        for _ in 0..6 {
            for layer in 1..layers.len() {
                let previous: BTreeMap<_, _> = layers[layer - 1]
                    .iter()
                    .enumerate()
                    .map(|(i, id)| (id.clone(), i as f32))
                    .collect();
                layers[layer].sort_by(|a, b| {
                    let score = |id: &String| {
                        let v: Vec<_> = graph[id].iter().filter_map(|n| previous.get(n)).collect();
                        if v.is_empty() {
                            0.
                        } else {
                            v.iter().copied().sum::<f32>() / v.len() as f32
                        }
                    };
                    score(a).total_cmp(&score(b)).then(a.cmp(b))
                });
            }
        }
        // Pack the connected region into a balanced grid, then exchange cells
        // to shorten actual adjacency. A broad hub no longer creates a very
        // tall column of neighbors with empty space on either side.
        let mut order: Vec<_> = layers.into_iter().flatten().collect();
        let columns = (order.len() as f32).sqrt().ceil().max(1.) as usize;
        let cells: Vec<_> = (0..order.len())
            .map(|i| ((i % columns) as i32, (i / columns) as i32))
            .collect();
        let mut index: BTreeMap<_, _> = order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();
        let distance =
            |a: usize, b: usize| (cells[a].0 - cells[b].0).abs() + (cells[a].1 - cells[b].1).abs();
        for _ in 0..5 {
            let mut changed = false;
            for a in 0..order.len() {
                if cancelled.load(Ordering::Relaxed) {
                    return None;
                }
                for b in a + 1..(a + 65).min(order.len()) {
                    let score =
                        |node: &String, at: usize, other: &String, other_at: usize| -> i32 {
                            let physical: i32 = graph[node]
                                .iter()
                                .map(|n| distance(at, if n == other { other_at } else { index[n] }))
                                .sum();
                            let direction: i32 = directed
                                .get(node)
                                .into_iter()
                                .flatten()
                                .map(|(n, outgoing)| {
                                    let neighbor = if n == other { other_at } else { index[n] };
                                    let delta = if *outgoing {
                                        cells[at].0 - cells[neighbor].0
                                    } else {
                                        cells[neighbor].0 - cells[at].0
                                    };
                                    (delta + 1).max(0) * 3
                                })
                                .sum();
                            physical + direction
                        };
                    let old = score(&order[a], a, &order[b], b) + score(&order[b], b, &order[a], a);
                    let new = score(&order[a], b, &order[b], a) + score(&order[b], a, &order[a], b);
                    if new < old {
                        order.swap(a, b);
                        index.insert(order[a].clone(), a);
                        index.insert(order[b].clone(), b);
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        let mut y = island_top;
        for row in order.chunks(columns) {
            let height = row
                .iter()
                .map(|id| {
                    ((HEADER + port_counts.get(id).copied().unwrap_or(0).max(1) as f32 * ROW + 24.)
                        / GRID)
                        .ceil()
                        * GRID
                })
                .fold(0., f32::max);
            for (x, id) in row.iter().enumerate() {
                state.positions.insert(
                    id.clone(),
                    Point {
                        x: x as f32 * 448.,
                        y,
                    },
                );
            }
            y += height + 80.;
        }
        island_top = y + 128.;
    }
    Some(state)
}

type Cell = (i32, i32);
fn cell(p: Point) -> Cell {
    ((p.x / GRID).round() as i32, (p.y / GRID).round() as i32)
}
fn point(c: Cell) -> Point {
    Point {
        x: c.0 as f32 * GRID,
        y: c.1 as f32 * GRID,
    }
}
fn edge(a: Cell, b: Cell) -> (Cell, Cell) {
    if a < b { (a, b) } else { (b, a) }
}
fn path(
    start: Cell,
    goal: Cell,
    blocked: &BTreeSet<Cell>,
    occupied: &BTreeMap<(Cell, Cell), String>,
    net: &str,
    bounds: (Cell, Cell),
    cancelled: &AtomicBool,
) -> Option<Vec<Cell>> {
    if blocked.contains(&start) || blocked.contains(&goal) {
        return None;
    }
    let mut heap = BinaryHeap::new();
    let mut cost = BTreeMap::new();
    let mut parent = BTreeMap::new();
    // Heading is part of the state so bend penalties remain well-defined.
    let begin = (start, 4usize);
    cost.insert(begin, 0i32);
    heap.push(Reverse((0i32, 0i32, begin)));
    let steps = [(1, 0), (0, 1), (-1, 0), (0, -1)];
    while let Some(Reverse((_, g, current))) = heap.pop() {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        if cost.get(&current) != Some(&g) {
            continue;
        }
        if current.0 == goal {
            let mut result = vec![goal];
            let mut at = current;
            while at != begin {
                at = parent[&at];
                result.push(at.0);
            }
            result.reverse();
            return Some(result);
        }
        for (dir, delta) in steps.iter().enumerate() {
            let next = (current.0.0 + delta.0, current.0.1 + delta.1);
            if next.0 < bounds.0.0
                || next.1 < bounds.0.1
                || next.0 > bounds.1.0
                || next.1 > bounds.1.1
                || blocked.contains(&next)
            {
                continue;
            }
            let shared = occupied.get(&edge(current.0, next));
            let step = if shared.is_some_and(|owner| owner == net) {
                4
            } else {
                10
            };
            let candidate = g
                + step
                + if current.1 != dir && current.1 != 4 {
                    12
                } else {
                    0
                }
                + if shared.is_some_and(|owner| owner != net) {
                    160
                } else {
                    0
                };
            let key = (next, dir);
            if cost.get(&key).is_none_or(|old| candidate < *old) {
                cost.insert(key, candidate);
                parent.insert(key, current);
                let h = (next.0 - goal.0).abs() + (next.1 - goal.1).abs();
                heap.push(Reverse((candidate + h * 4, candidate, key)));
            }
        }
    }
    None
}

pub fn route(d: &SystemDescription, state: &DiagramState) -> Layout {
    route_cancellable(d, state, &AtomicBool::new(false)).expect("not cancelled")
}
pub fn route_cancellable(
    d: &SystemDescription,
    state: &DiagramState,
    cancelled: &AtomicBool,
) -> Option<Layout> {
    if cancelled.load(Ordering::Relaxed) {
        return None;
    }
    let mut sides = BTreeMap::new();
    let mut ports = BTreeMap::new();
    let mut nodes = BTreeMap::new();
    let mut neighbors: BTreeMap<String, Vec<f32>> = BTreeMap::new();
    for net in d.nets.values() {
        for id in &net.ports {
            if cancelled.load(Ordering::Relaxed) {
                return None;
            }
            for peer in &net.ports {
                if let (Some(a), Some(b)) = (d.ports.get(id), d.ports.get(peer)) {
                    if a.component != b.component {
                        if let Some(p) = state.positions.get(&b.component) {
                            neighbors.entry(id.clone()).or_default().push(p.x);
                        }
                    }
                }
            }
        }
    }
    let terminal_list = leaves(d);
    for (id, position) in &state.positions {
        let mut left = vec![];
        let mut right = vec![];
        for p in terminal_list.iter().filter(|p| &p.component == id) {
            let side = match &p.schema {
                PortKind::SignalInput { .. } => Side::Left,
                PortKind::SignalOutput { .. } => Side::Right,
                _ => {
                    let xs = neighbors.get(&p.id);
                    if xs.is_some_and(|v| v.iter().sum::<f32>() / (v.len() as f32) < position.x) {
                        Side::Left
                    } else {
                        Side::Right
                    }
                }
            };
            sides.insert(p.id.clone(), side);
            if side == Side::Left {
                left.push(*p)
            } else {
                right.push(*p)
            }
        }
        let height =
            ((HEADER + left.len().max(right.len()).max(1) as f32 * ROW + 16.) / GRID).ceil() * GRID;
        nodes.insert(
            id.clone(),
            NodeBox {
                position: *position,
                height,
            },
        );
        for (list, side) in [(left, Side::Left), (right, Side::Right)] {
            for (i, p) in list.iter().enumerate() {
                ports.insert(
                    p.id.clone(),
                    Point {
                        x: position.x + if side == Side::Left { 0. } else { WIDTH },
                        y: position.y + HEADER + i as f32 * ROW,
                    },
                );
            }
        }
    }
    let mut min = Point { x: 0., y: 0. };
    let mut max = Point { x: WIDTH, y: 160. };
    for n in nodes.values() {
        min.x = min.x.min(n.position.x);
        min.y = min.y.min(n.position.y);
        max.x = max.x.max(n.position.x + WIDTH);
        max.y = max.y.max(n.position.y + n.height);
    }
    let lo = cell(Point {
        x: min.x - 160.,
        y: min.y - 160.,
    });
    let hi = cell(Point {
        x: max.x + 160.,
        y: max.y + 160.,
    });
    let mut blocked = BTreeSet::new();
    for n in nodes.values() {
        let a = cell(n.position);
        let b = cell(Point {
            x: n.position.x + WIDTH,
            y: n.position.y + n.height,
        });
        for x in a.0..=b.0 {
            for y in a.1..=b.1 {
                blocked.insert((x, y));
            }
        }
    }
    let leads: BTreeMap<_, _> = ports
        .iter()
        .map(|(id, p)| {
            (
                id.clone(),
                cell(Point {
                    x: p.x + sides[id].sign() * 32.,
                    y: p.y,
                }),
            )
        })
        .collect();
    let mut order: Vec<_> = d.nets.values().collect();
    order.sort_by_key(|n| (n.ports.len(), n.id.clone()));
    let mut occupied = BTreeMap::new();
    let mut nets = BTreeMap::new();
    let mut unrouted = vec![];
    for net in order {
        if cancelled.load(Ordering::Relaxed) {
            return None;
        }
        let ids: Vec<_> = net
            .ports
            .iter()
            .filter(|p| leads.contains_key(*p))
            .collect();
        if ids.is_empty() {
            continue;
        }
        let mut xs: Vec<_> = ids.iter().map(|p| leads[*p].0).collect();
        let mut ys: Vec<_> = ids.iter().map(|p| leads[*p].1).collect();
        xs.sort();
        ys.sort();
        let median = (xs[xs.len() / 2], ys[ys.len() / 2]);
        let mut hub = median;
        if blocked.contains(&hub) {
            'search: for radius in 1..=((hi.0 - lo.0) + (hi.1 - lo.1)) {
                for dx in -radius..=radius {
                    for dy in [radius - dx.abs(), -(radius - dx.abs())] {
                        let c = (median.0 + dx, median.1 + dy);
                        if !blocked.contains(&c)
                            && c.0 >= lo.0
                            && c.0 <= hi.0
                            && c.1 >= lo.1
                            && c.1 <= hi.1
                        {
                            hub = c;
                            break 'search;
                        }
                    }
                }
            }
        }
        let mut branches = vec![];
        let mut network: BTreeMap<Cell, BTreeSet<Cell>> = BTreeMap::new();
        for id in ids {
            let start = leads[id];
            if let Some(cells) = path(
                start,
                hub,
                &blocked,
                &occupied,
                &net.id,
                (lo, hi),
                cancelled,
            ) {
                for pair in cells.windows(2) {
                    occupied.insert(edge(pair[0], pair[1]), net.id.clone());
                    network.entry(pair[0]).or_default().insert(pair[1]);
                    network.entry(pair[1]).or_default().insert(pair[0]);
                }
                let p = ports[id];
                let lead = point(start);
                let mut branch = vec![p, Point { x: lead.x, y: p.y }];
                branch.extend(cells.into_iter().map(point));
                let mut simplified: Vec<Point> = Vec::new();
                for p in branch {
                    if simplified.last() == Some(&p) {
                        continue;
                    }
                    while simplified.len() >= 2 {
                        let a = simplified[simplified.len() - 2];
                        let b = simplified[simplified.len() - 1];
                        if (a.x == b.x && b.x == p.x) || (a.y == b.y && b.y == p.y) {
                            simplified.pop();
                        } else {
                            break;
                        }
                    }
                    simplified.push(p);
                }
                branches.push(simplified);
            } else {
                unrouted.push(format!("{}: {}", net.id, id));
            }
        }
        let junctions = network
            .iter()
            .filter(|(_, neighbors)| neighbors.len() > 2)
            .map(|(c, _)| point(*c))
            .collect();
        nets.insert(
            net.id.clone(),
            NetRoute {
                junction: point(hub),
                junctions,
                branches,
            },
        );
    }
    for net in nets.values() {
        for p in net.branches.iter().flatten() {
            min.x = min.x.min(p.x);
            min.y = min.y.min(p.y);
            max.x = max.x.max(p.x);
            max.y = max.y.max(p.y);
        }
    }
    Some(Layout {
        ports,
        nets,
        nodes,
        sides,
        bounds: [min, max],
        unrouted,
    })
}
