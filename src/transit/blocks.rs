//! The text of the transport files as openOMSI reads it: `[block]` lines, each followed by
//! `key value` lines; a key may repeat, `//` and `#` start comments. A file is kept as its
//! blocks with their keys in order, so what openMapper does not edit itself (keys a newer
//! openOMSI knows, a modder's own) is written back as it was. Comments are not kept: a file is
//! written again only when something in it was changed here.

/// One `[block]` and its `key value` lines.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Block {
    /// The keyword, lower-cased, without brackets.
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl Block {
    pub fn new(name: &str) -> Block {
        Block { name: name.to_ascii_lowercase(), entries: Vec::new() }
    }

    /// With `key value` lines (empty values left out).
    pub fn with(name: &str, entries: &[(&str, &str)]) -> Block {
        let mut b = Block::new(name);
        for (k, v) in entries {
            if !v.trim().is_empty() {
                b.entries.push((k.to_string(), v.trim().to_string()));
            }
        }
        b
    }

    /// The first value of a key.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.iter().find(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str())
    }

    pub fn str(&self, key: &str) -> String {
        self.get(key).unwrap_or("").to_string()
    }

    /// Every value of a repeatable key.
    pub fn all(&self, key: &str) -> Vec<&str> {
        self.entries.iter().filter(|(k, _)| k.eq_ignore_ascii_case(key)).map(|(_, v)| v.as_str()).collect()
    }

    /// Set a key's (first) value where it stands; an empty value takes the key out. (Kept as
    /// typed, so a name can be typed with its spaces; the file gets it trimmed.)
    pub fn set(&mut self, key: &str, value: &str) {
        let value = if value.trim().is_empty() { "" } else { value };
        match self.entries.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)) {
            Some(i) if value.is_empty() => {
                self.entries.remove(i);
            }
            Some(i) => self.entries[i].1 = value.to_string(),
            None if value.is_empty() => {}
            None => self.entries.push((key.to_ascii_lowercase(), value.to_string())),
        }
    }

    /// A repeatable key's values, all of them replaced (in the place of the first).
    pub fn set_all(&mut self, key: &str, values: &[String]) {
        let at = self.entries.iter().position(|(k, _)| k.eq_ignore_ascii_case(key)).unwrap_or(self.entries.len());
        self.entries.retain(|(k, _)| !k.eq_ignore_ascii_case(key));
        let at = at.min(self.entries.len());
        for (i, v) in values.iter().filter(|v| !v.trim().is_empty()).enumerate() {
            self.entries.insert(at + i, (key.to_ascii_lowercase(), v.trim().to_string()));
        }
    }

    pub fn id(&self) -> String {
        self.str("id")
    }

    /// A number at the start of a value (`150 m` is 150).
    pub fn num(&self, key: &str) -> Option<f64> {
        let v = self.get(key)?.trim();
        let end = v.find(|c: char| !(c.is_ascii_digit() || matches!(c, '.' | ',' | '-' | '+'))).unwrap_or(v.len());
        v[..end].replace(',', ".").parse().ok()
    }

    /// Three numbers `x y z`.
    pub fn vec3(&self, key: &str) -> Option<[f64; 3]> {
        vec3(self.get(key)?)
    }

    /// A yes / no value.
    pub fn flag(&self, key: &str) -> Option<bool> {
        match self.get(key)?.trim().to_ascii_lowercase().as_str() {
            "1" | "yes" | "true" | "on" => Some(true),
            "0" | "no" | "false" | "off" => Some(false),
            _ => None,
        }
    }

    /// A list value split at commas and semicolons (names may hold spaces).
    pub fn names(&self, key: &str) -> Vec<String> {
        self.all(key).iter().flat_map(|v| v.split([',', ';'])).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }

    /// A list value split at commas, semicolons and spaces.
    pub fn words(&self, key: &str) -> Vec<String> {
        self.all(key).iter().flat_map(|v| v.split(|c: char| c == ',' || c == ';' || c.is_whitespace())).map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    }
}

pub fn vec3(v: &str) -> Option<[f64; 3]> {
    let n: Vec<f64> = v.split_whitespace().filter_map(|w| w.parse().ok()).collect();
    (n.len() >= 3).then(|| [n[0], n[1], n[2]])
}

/// A point as the files write it.
pub fn point(p: [f64; 3]) -> String {
    format!("{} {} {}", short(p[0]), short(p[1]), short(p[2]))
}

/// A number without needless decimals (centimetres at most).
pub fn short(v: f64) -> String {
    let s = format!("{:.2}", v);
    let s = s.trim_end_matches('0').trim_end_matches('.');
    if s == "-0" { "0".into() } else { s.to_string() }
}

/// A transport file: its blocks in order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct BlockFile {
    pub blocks: Vec<Block>,
}

impl BlockFile {
    pub fn parse(text: &str) -> BlockFile {
        let mut blocks: Vec<Block> = Vec::new();
        for raw in text.lines() {
            let line = raw.trim().trim_start_matches('\u{feff}');
            if line.is_empty() || line.starts_with('#') || line.starts_with("//") {
                continue;
            }
            if let Some(rest) = line.strip_prefix('[') {
                if let Some(end) = rest.find(']') {
                    blocks.push(Block::new(rest[..end].trim()));
                }
                continue;
            }
            let Some(b) = blocks.last_mut() else { continue };
            // (a trailing `// comment` is cut off, as the game cuts it)
            let body = line.find(" //").map(|p| line[..p].trim_end()).unwrap_or(line);
            let (k, v) = body.split_once(char::is_whitespace).map(|(k, v)| (k.trim(), v.trim())).unwrap_or((body, ""));
            b.entries.push((k.to_ascii_lowercase(), v.to_string()));
        }
        BlockFile { blocks }
    }

    pub fn text(&self, title: &str) -> String {
        let mut s = format!("// {title}\n// openOMSI's transport folder (docs/TRANSPORT_FORMATS.md), written by openMapper\n");
        for b in &self.blocks {
            s.push('\n');
            s.push_str(&format!("[{}]\n", b.name));
            for (k, v) in &b.entries {
                let v = v.trim();
                if v.is_empty() {
                    s.push_str(&format!("{k}\n"));
                } else {
                    s.push_str(&format!("{k} {v}\n"));
                }
            }
        }
        s
    }

    pub fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }

    /// The indexes of the blocks called `name`.
    pub fn of<'a>(&'a self, name: &'a str) -> impl Iterator<Item = usize> + 'a {
        self.blocks.iter().enumerate().filter(move |(_, b)| b.name == name).map(|(i, _)| i)
    }

    /// The block `name` with `id` (without regard to case).
    pub fn find(&self, name: &str, id: &str) -> Option<usize> {
        self.of(name).find(|&i| self.blocks[i].id().eq_ignore_ascii_case(id.trim()))
    }

    /// The blocks that belong to block `i`: those after it up to the next block that is no
    /// child (`children` names the kinds that belong to the one before them).
    pub fn children(&self, i: usize, children: &[&str]) -> std::ops::Range<usize> {
        let end = (i + 1..self.blocks.len()).find(|&j| !children.contains(&self.blocks[j].name.as_str())).unwrap_or(self.blocks.len());
        i + 1..end
    }

    /// Take out block `i` with the blocks that belong to it.
    pub fn remove_with(&mut self, i: usize, children: &[&str]) {
        let r = self.children(i, children);
        self.blocks.drain(i..r.end);
    }

    /// Put `b` last among the blocks that belong to block `parent`; its index.
    pub fn add_child(&mut self, parent: usize, b: Block, children: &[&str]) -> usize {
        let at = self.children(parent, children).end;
        self.blocks.insert(at, b);
        at
    }

    /// An id `base`, `base 2`, … not used by any block `name` yet.
    pub fn free_id(&self, name: &str, base: &str) -> String {
        free_id(base, |id| self.find(name, id).is_some())
    }
}

/// `base`, else `base_2`, `base_3`, … whichever `taken` says is free.
pub fn free_id(base: &str, taken: impl Fn(&str) -> bool) -> String {
    let base: String = base.trim().chars().map(|c| if c.is_whitespace() { '_' } else { c }).collect();
    let base = if base.is_empty() { "id".to_string() } else { base };
    if !taken(&base) {
        return base;
    }
    (2..).map(|n| format!("{base}_{n}")).find(|id| !taken(id)).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_keep_what_is_not_edited() {
        let f = BlockFile::parse("// a map's stations\n[Station]\nid A\nname Main Square  // the hub\nfuture_key 7\n[platform]\nid 1\n[station]\nid B\n");
        assert_eq!(f.blocks.len(), 3);
        assert_eq!(f.blocks[0].get("NAME"), Some("Main Square"));
        assert_eq!(f.children(0, &["platform"]), 1..2);
        let again = BlockFile::parse(&f.text("stations"));
        assert_eq!(again, f);
        assert!(f.text("stations").contains("future_key 7"));
    }

    #[test]
    fn keys_are_set_and_taken_out_in_place() {
        let mut b = Block::with("segment", &[("id", "s1"), ("via", "A, B"), ("dwell", "30 s")]);
        b.set("via", "C");
        b.set("dwell", "");
        b.set("speed", "60 km/h");
        assert_eq!(b.entries, vec![("id".into(), "s1".into()), ("via".into(), "C".into()), ("speed".into(), "60 km/h".into())]);
        b.set_all("p", &["0 0 6".into(), "10 0 6".into()]);
        assert_eq!(b.all("p"), vec!["0 0 6", "10 0 6"]);
        assert_eq!(b.num("speed"), Some(60.0));
        assert_eq!(short(6.0), "6");
        assert_eq!(short(-0.001), "0");
        assert_eq!(short(5.126), "5.13");
    }

    #[test]
    fn children_are_added_and_removed_with_their_parent() {
        let mut f = BlockFile::parse("[route]\nid R\n[segment]\nid s\n[route]\nid Q\n");
        f.add_child(0, Block::with("service", &[("id", "all")]), &["segment", "service", "trip"]);
        assert_eq!(f.blocks[2].name, "service");
        f.remove_with(0, &["segment", "service", "trip"]);
        assert_eq!(f.blocks.len(), 1);
        assert_eq!(f.free_id("route", "Q"), "Q_2");
    }
}
