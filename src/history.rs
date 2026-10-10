//! Undo and redo: every edit keeps the tiles it changed (and the global settings) as they
//! were before it, and stepping back swaps them in again.

use std::collections::BTreeMap;

use omsi_map::GlobalCfg;

use crate::doc::{next_rev, Document, TileDoc, TileKey};

#[derive(Clone)]
struct Step {
    label: String,
    /// Each tile before the edit (None: it did not exist).
    tiles: BTreeMap<TileKey, Option<TileDoc>>,
    global: Option<(GlobalCfg, Vec<TileKey>)>,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
    open: Option<Step>,
}

const LIMIT: usize = 100;

impl History {
    /// Start an edit (ends one still open).
    pub fn begin(&mut self, label: &str) {
        self.commit();
        self.open = Some(Step { label: label.to_string(), tiles: BTreeMap::new(), global: None });
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// Keep a tile as it is now, before the open edit changes it (once per edit).
    pub fn touch_tile(&mut self, doc: &Document, k: TileKey) {
        if let Some(s) = &mut self.open {
            s.tiles.entry(k).or_insert_with(|| doc.tiles.get(&k).cloned());
        }
    }

    /// Keep the global settings and the tile list.
    pub fn touch_global(&mut self, doc: &Document) {
        if let Some(s) = &mut self.open {
            if s.global.is_none() {
                s.global = Some((doc.global.clone(), doc.order.clone()));
            }
        }
    }

    /// Close the open edit (nothing kept: it is dropped).
    pub fn commit(&mut self) {
        if let Some(s) = self.open.take() {
            if s.tiles.is_empty() && s.global.is_none() {
                return;
            }
            self.undo.push(s);
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.open.is_some()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn undo(&mut self, doc: &mut Document) {
        self.commit();
        if let Some(s) = self.undo.pop() {
            let back = apply(doc, s);
            self.redo.push(back);
        }
    }

    pub fn redo(&mut self, doc: &mut Document) {
        self.commit();
        if let Some(s) = self.redo.pop() {
            let back = apply(doc, s);
            self.undo.push(back);
        }
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = None;
    }
}

/// Put a step's state into the document; returns the state it replaced (the opposite step).
fn apply(doc: &mut Document, s: Step) -> Step {
    let mut back = Step { label: s.label.clone(), tiles: BTreeMap::new(), global: None };
    for (k, t) in s.tiles {
        let now = match t {
            Some(mut t) => {
                // new revisions: whatever shows the tile builds it again
                t.terrain_rev = next_rev();
                t.mask_rev = next_rev();
                t.content_rev = next_rev();
                t.dirty = true;
                doc.tiles.insert(k, t)
            }
            None => doc.tiles.remove(&k),
        };
        back.tiles.insert(k, now);
    }
    if let Some((g, order)) = s.global {
        back.global = Some((std::mem::replace(&mut doc.global, g), std::mem::replace(&mut doc.order, order)));
        doc.global_rev = next_rev();
    }
    doc.global_dirty = true;
    back
}
