//! Undo and redo: every edit keeps the tiles it changed (and the global settings, the
//! timetable) as they were before it, and stepping back swaps them in again.

use std::collections::BTreeMap;

use omsi_map::GlobalCfg;

use crate::doc::{next_rev, Document, TileDoc, TileKey};

#[derive(Clone)]
struct Step {
    label: String,
    /// Each tile before the edit (None: it did not exist).
    tiles: BTreeMap<TileKey, Option<TileDoc>>,
    global: Option<(GlobalCfg, Vec<TileKey>)>,
    timetable: Option<crate::timetable::Timetable>,
    traffic: Option<crate::traffic::Traffic>,
    descriptions: Option<crate::descriptions::Descriptions>,
    /// When it was made (edits of one kind in quick succession are one step).
    at: std::time::Instant,
}

/// What a window's edit changed, as it was before (see `History::push_snapshot`).
#[derive(Default)]
pub struct Snapshot {
    pub global: Option<(GlobalCfg, Vec<TileKey>)>,
    pub timetable: Option<crate::timetable::Timetable>,
    pub traffic: Option<crate::traffic::Traffic>,
    pub descriptions: Option<crate::descriptions::Descriptions>,
}

/// Snapshots of one kind closer together than this make one step (typing a name, dragging
/// a value).
const MERGE: std::time::Duration = std::time::Duration::from_millis(1500);

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
        self.open = Some(Step { label: label.to_string(), tiles: BTreeMap::new(), global: None, timetable: None, traffic: None, descriptions: None, at: std::time::Instant::now() });
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
            if s.tiles.is_empty() && s.global.is_none() && s.timetable.is_none() && s.traffic.is_none() && s.descriptions.is_none() {
                return;
            }
            self.undo.push(s);
            if self.undo.len() > LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
    }

    /// A finished edit of a window, given as the state before it: one undo step, or part of
    /// the last one when that was of the same kind a moment ago.
    pub fn push_snapshot(&mut self, label: &str, before: Snapshot) {
        self.commit();
        if let Some(last) = self.undo.last_mut() {
            let same = last.label == label && last.tiles.is_empty() && last.global.is_some() == before.global.is_some() && last.timetable.is_some() == before.timetable.is_some() && last.traffic.is_some() == before.traffic.is_some() && last.descriptions.is_some() == before.descriptions.is_some();
            if same && self.redo.is_empty() && last.at.elapsed() < MERGE {
                last.at = std::time::Instant::now();
                return;
            }
        }
        if before.global.is_none() && before.timetable.is_none() && before.traffic.is_none() && before.descriptions.is_none() {
            return;
        }
        self.undo.push(Step { label: label.to_string(), tiles: BTreeMap::new(), global: before.global, timetable: before.timetable, traffic: before.traffic, descriptions: before.descriptions, at: std::time::Instant::now() });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
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

    /// The object files the tiles kept for undo and redo name (what a step can bring back).
    pub fn object_files(&self) -> impl Iterator<Item = &str> {
        self.undo.iter().chain(&self.redo).chain(&self.open).flat_map(|s| s.tiles.values().flatten()).flat_map(|t| t.tile.objects.iter().map(|o| o.file.as_str()))
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.open = None;
    }
}

/// Put a step's state into the document; returns the state it replaced (the opposite step).
fn apply(doc: &mut Document, s: Step) -> Step {
    let mut back = Step { label: s.label.clone(), tiles: BTreeMap::new(), global: None, timetable: None, traffic: None, descriptions: None, at: s.at };
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
        // (IDCodes given out stay given out: junction files are named by them)
        let next = doc.global.next_id_code.max(g.next_id_code);
        back.global = Some((std::mem::replace(&mut doc.global, g), std::mem::replace(&mut doc.order, order)));
        doc.global.next_id_code = next;
        doc.global_rev = next_rev();
    }
    if let Some(t) = s.timetable {
        let old = std::mem::replace(&mut doc.timetable, t);
        doc.timetable.restored(&old);
        back.timetable = Some(old);
    }
    if let Some(t) = s.traffic {
        back.traffic = Some(std::mem::replace(&mut doc.traffic, t));
        doc.traffic.restored();
    }
    if let Some(d) = s.descriptions {
        let old = std::mem::replace(&mut doc.descriptions, d);
        doc.descriptions.restored(&old);
        back.descriptions = Some(old);
    }
    doc.global_dirty = true;
    back
}

#[cfg(test)]
mod tests {
    use super::*;

    /// IDCodes handed out stay used after an undo: a junction made again would else get the
    /// number of the one undone and write over its files (which a saved map may still name).
    #[test]
    fn undo_does_not_hand_out_an_idcode_again() {
        let mut doc = crate::selection::tests::flat_map(1);
        let mut h = History::default();
        h.begin("t");
        h.touch_tile(&doc, (0, 0));
        h.touch_global(&doc);
        let first = doc.new_id();
        h.commit();
        h.undo(&mut doc);
        assert!(doc.new_id() > first);
        h.redo(&mut doc);
        assert!(doc.new_id() > first + 1);
    }
}
