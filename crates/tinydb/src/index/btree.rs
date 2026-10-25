use std::collections::HashSet;
use std::io::{self, ErrorKind};
use std::ops::{Bound, RangeBounds};
use std::path::Path;

use crate::storage::{PAGE_SIZE, Page, PageId, Pager, RecordId};

const HEADER_SIZE: usize = 16;
const LEAF_CAPACITY: usize = (PAGE_SIZE - HEADER_SIZE) / 18;
const INTERNAL_CAPACITY: usize = (PAGE_SIZE - HEADER_SIZE - 8) / 16;
const ROOT: PageId = 1;

struct Node {
    keys: Vec<i64>,
    contents: Contents,
}

enum Contents {
    Leaf {
        records: Vec<RecordId>,
        next: PageId,
    },
    Internal {
        children: Vec<PageId>,
    },
}

impl Node {
    fn capacity(&self) -> usize {
        match self.contents {
            Contents::Leaf { .. } => LEAF_CAPACITY,
            Contents::Internal { .. } => INTERNAL_CAPACITY,
        }
    }

    fn encode(&self) -> Page {
        let mut bytes = [0; PAGE_SIZE];
        bytes[4..6].copy_from_slice(&(self.keys.len() as u16).to_le_bytes());
        match &self.contents {
            Contents::Leaf { records, next } => {
                bytes[..4].copy_from_slice(b"BTL1");
                bytes[8..16].copy_from_slice(&next.to_le_bytes());
                for (index, (key, record)) in self.keys.iter().zip(records).enumerate() {
                    let offset = HEADER_SIZE + index * 18;
                    bytes[offset..offset + 8].copy_from_slice(&key.to_le_bytes());
                    bytes[offset + 8..offset + 16].copy_from_slice(&record.page_id.to_le_bytes());
                    bytes[offset + 16..offset + 18].copy_from_slice(&record.slot_id.to_le_bytes());
                }
            }
            Contents::Internal { children } => {
                bytes[..4].copy_from_slice(b"BTI1");
                bytes[16..24].copy_from_slice(&children[0].to_le_bytes());
                for (index, key) in self.keys.iter().enumerate() {
                    let offset = HEADER_SIZE + 8 + index * 16;
                    bytes[offset..offset + 8].copy_from_slice(&key.to_le_bytes());
                    bytes[offset + 8..offset + 16]
                        .copy_from_slice(&children[index + 1].to_le_bytes());
                }
            }
        }
        bytes
    }

    fn decode(bytes: &Page, page_count: u64) -> io::Result<Self> {
        let count = usize::from(u16::from_le_bytes(bytes[4..6].try_into().unwrap()));
        if bytes[6..8] != [0; 2] {
            return Err(invalid_data("nonzero index-node reserved bytes"));
        }
        let mut keys = Vec::with_capacity(count.min(INTERNAL_CAPACITY));
        let (contents, end) = match &bytes[..4] {
            b"BTL1" if count <= LEAF_CAPACITY => {
                let next = u64::from_le_bytes(bytes[8..16].try_into().unwrap());
                if next > page_count {
                    return Err(invalid_data("leaf link exceeds allocated pages"));
                }
                let mut records = Vec::with_capacity(count);
                for index in 0..count {
                    let offset = HEADER_SIZE + index * 18;
                    keys.push(i64::from_le_bytes(
                        bytes[offset..offset + 8].try_into().unwrap(),
                    ));
                    let page_id =
                        u64::from_le_bytes(bytes[offset + 8..offset + 16].try_into().unwrap());
                    let slot_id =
                        u16::from_le_bytes(bytes[offset + 16..offset + 18].try_into().unwrap());
                    if page_id == 0 {
                        return Err(invalid_data("index record points to metadata page"));
                    }
                    records.push(RecordId { page_id, slot_id });
                }
                (Contents::Leaf { records, next }, HEADER_SIZE + count * 18)
            }
            b"BTI1" if count <= INTERNAL_CAPACITY => {
                if bytes[8..16] != [0; 8] {
                    return Err(invalid_data("nonzero internal-node reserved bytes"));
                }
                let mut children = Vec::with_capacity(count + 1);
                children.push(u64::from_le_bytes(bytes[16..24].try_into().unwrap()));
                for index in 0..count {
                    let offset = HEADER_SIZE + 8 + index * 16;
                    keys.push(i64::from_le_bytes(
                        bytes[offset..offset + 8].try_into().unwrap(),
                    ));
                    children.push(u64::from_le_bytes(
                        bytes[offset + 8..offset + 16].try_into().unwrap(),
                    ));
                }
                if children
                    .iter()
                    .any(|&child| child == 0 || child > page_count)
                {
                    return Err(invalid_data("index child is outside allocated pages"));
                }
                (
                    Contents::Internal { children },
                    HEADER_SIZE + 8 + count * 16,
                )
            }
            _ => return Err(invalid_data("invalid index-node magic or capacity")),
        };
        if keys.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invalid_data("index keys are not strictly ordered"));
        }
        if bytes[end..].iter().any(|&byte| byte != 0) {
            return Err(invalid_data("nonzero unused index-node bytes"));
        }
        Ok(Self { keys, contents })
    }
}

pub struct BPlusTree {
    pager: Pager,
}

impl BPlusTree {
    pub fn create(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut tree = Self {
            pager: Pager::create(path)?,
        };
        tree.allocate_node(&Node {
            keys: Vec::new(),
            contents: Contents::Leaf {
                records: Vec::new(),
                next: 0,
            },
        })?;
        Ok(tree)
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let mut tree = Self {
            pager: Pager::open(path)?,
        };
        if tree.pager.page_count() == 0 {
            return Err(invalid_data("index root is missing"));
        }
        let mut visited = HashSet::new();
        let mut leaves = Vec::new();
        tree.validate_subtree(ROOT, &mut visited, &mut leaves, 0)?;
        for (index, &(_, next)) in leaves.iter().enumerate() {
            let expected = leaves.get(index + 1).map_or(0, |&(page_id, _)| page_id);
            if next != expected {
                return Err(invalid_data("leaf links do not match tree order"));
            }
        }
        for page_id in 1..=tree.pager.page_count() {
            if !visited.contains(&page_id) {
                let bytes = tree.pager.read_page(page_id)?;
                if &bytes[..4] != b"BTF1" || bytes[4..].iter().any(|&byte| byte != 0) {
                    return Err(invalid_data(
                        "unreachable live or malformed retired index page",
                    ));
                }
            }
        }
        Ok(tree)
    }

    pub fn get(&mut self, key: i64) -> io::Result<Option<RecordId>> {
        let mut page_id = ROOT;
        loop {
            let node = self.read_node(page_id)?;
            match node.contents {
                Contents::Leaf { records, .. } => {
                    return Ok(node
                        .keys
                        .binary_search(&key)
                        .ok()
                        .map(|index| records[index]));
                }
                Contents::Internal { children } => {
                    page_id = children[node.keys.partition_point(|&separator| separator <= key)];
                }
            }
        }
    }

    pub fn insert(&mut self, key: i64, record: RecordId) -> io::Result<()> {
        if record.page_id == 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidInput,
                "record page ID must be nonzero",
            ));
        }
        if let Some((separator, right)) = self.insert_into(ROOT, key, record)? {
            let left_node = self.read_node(ROOT)?;
            let left = self.allocate_node(&left_node)?;
            self.write_node(
                ROOT,
                &Node {
                    keys: vec![separator],
                    contents: Contents::Internal {
                        children: vec![left, right],
                    },
                },
            )?;
        }
        Ok(())
    }

    pub fn range(&mut self, bounds: impl RangeBounds<i64>) -> io::Result<Vec<(i64, RecordId)>> {
        let mut page_id = ROOT;
        loop {
            let node = self.read_node(page_id)?;
            match node.contents {
                Contents::Internal { children } => {
                    let child = match bounds.start_bound() {
                        Bound::Included(key) | Bound::Excluded(key) => {
                            node.keys.partition_point(|separator| separator <= key)
                        }
                        Bound::Unbounded => 0,
                    };
                    page_id = children[child];
                }
                Contents::Leaf { .. } => break,
            }
        }
        let mut entries = Vec::new();
        loop {
            let node = self.read_node(page_id)?;
            let Contents::Leaf { records, next } = node.contents else {
                return Err(invalid_data("leaf link points to an internal node"));
            };
            let start = match bounds.start_bound() {
                Bound::Included(key) => node.keys.partition_point(|entry| entry < key),
                Bound::Excluded(key) => node.keys.partition_point(|entry| entry <= key),
                Bound::Unbounded => 0,
            };
            for (&key, &record) in node.keys[start..].iter().zip(&records[start..]) {
                let past_end = match bounds.end_bound() {
                    Bound::Included(end) => key > *end,
                    Bound::Excluded(end) => key >= *end,
                    Bound::Unbounded => false,
                };
                if past_end {
                    return Ok(entries);
                }
                entries.push((key, record));
            }
            if next == 0 {
                return Ok(entries);
            }
            page_id = next;
        }
    }

    pub fn delete(&mut self, key: i64) -> io::Result<RecordId> {
        let (record, _) = self.delete_from(ROOT, key)?;
        let root = self.read_node(ROOT)?;
        if let Contents::Internal { children } = root.contents
            && root.keys.is_empty()
        {
            let child = children[0];
            let replacement = self.read_node(child)?;
            self.write_node(ROOT, &replacement)?;
            self.retire_node(child)?;
        }
        Ok(record)
    }

    pub fn page_count(&self) -> u64 {
        self.pager.page_count()
    }

    pub fn sync(&self) -> io::Result<()> {
        self.pager.sync()
    }

    fn read_node(&mut self, page_id: PageId) -> io::Result<Node> {
        Node::decode(&self.pager.read_page(page_id)?, self.pager.page_count())
    }

    fn write_node(&mut self, page_id: PageId, node: &Node) -> io::Result<()> {
        self.pager.write_page(page_id, &node.encode())
    }

    fn allocate_node(&mut self, node: &Node) -> io::Result<PageId> {
        let page_id = self.pager.allocate_page()?;
        self.write_node(page_id, node)?;
        Ok(page_id)
    }

    fn retire_node(&mut self, page_id: PageId) -> io::Result<()> {
        let mut bytes = [0; PAGE_SIZE];
        bytes[..4].copy_from_slice(b"BTF1");
        self.pager.write_page(page_id, &bytes)
    }

    fn insert_into(
        &mut self,
        page_id: PageId,
        key: i64,
        record: RecordId,
    ) -> io::Result<Option<(i64, PageId)>> {
        let mut node = self.read_node(page_id)?;
        match &mut node.contents {
            Contents::Leaf { records, .. } => {
                let index = node.keys.binary_search(&key).map_or_else(Ok, |_| {
                    Err(io::Error::new(
                        ErrorKind::AlreadyExists,
                        "index key already exists",
                    ))
                })?;
                node.keys.insert(index, key);
                records.insert(index, record);
            }
            Contents::Internal { children } => {
                let index = node.keys.partition_point(|&separator| separator <= key);
                let Some((separator, right)) = self.insert_into(children[index], key, record)?
                else {
                    return Ok(None);
                };
                node.keys.insert(index, separator);
                children.insert(index + 1, right);
            }
        }
        if node.keys.len() <= node.capacity() {
            self.write_node(page_id, &node)?;
            return Ok(None);
        }
        let middle = node.keys.len() / 2;
        let separator;
        let right;
        match &mut node.contents {
            Contents::Leaf { records, next } => {
                let right_node = Node {
                    keys: node.keys.split_off(middle),
                    contents: Contents::Leaf {
                        records: records.split_off(middle),
                        next: *next,
                    },
                };
                separator = right_node.keys[0];
                right = self.allocate_node(&right_node)?;
                *next = right;
            }
            Contents::Internal { children } => {
                let right_node = Node {
                    keys: node.keys.split_off(middle + 1),
                    contents: Contents::Internal {
                        children: children.split_off(middle + 1),
                    },
                };
                separator = node.keys.pop().unwrap();
                right = self.allocate_node(&right_node)?;
            }
        }
        self.write_node(page_id, &node)?;
        Ok(Some((separator, right)))
    }

    fn delete_from(&mut self, page_id: PageId, key: i64) -> io::Result<(RecordId, Option<i64>)> {
        let mut node = self.read_node(page_id)?;
        let record;
        let mut new_minimum = None;
        match &mut node.contents {
            Contents::Leaf { records, .. } => {
                let index = node
                    .keys
                    .binary_search(&key)
                    .map_err(|_| io::Error::new(ErrorKind::NotFound, "index key does not exist"))?;
                node.keys.remove(index);
                record = records.remove(index);
                if index == 0 {
                    new_minimum = node.keys.first().copied();
                }
            }
            Contents::Internal { children } => {
                let index = node.keys.partition_point(|&separator| separator <= key);
                let (deleted, child_minimum) = self.delete_from(children[index], key)?;
                record = deleted;
                let mut child = self.read_node(children[index])?;
                if index == 0 {
                    new_minimum = child_minimum;
                } else if let Some(minimum) = child_minimum {
                    node.keys[index - 1] = minimum;
                }
                if child.keys.len() < child.capacity() / 2 {
                    let mut sibling_index = if index > 0 { index - 1 } else { index + 1 };
                    let mut sibling = self.read_node(children[sibling_index])?;
                    if sibling.keys.len() <= sibling.capacity() / 2
                        && index > 0
                        && index + 1 < children.len()
                    {
                        sibling_index = index + 1;
                        sibling = self.read_node(children[sibling_index])?;
                    }
                    let separator_index = index.min(sibling_index);
                    if sibling.keys.len() > sibling.capacity() / 2 {
                        match (&mut child.contents, &mut sibling.contents) {
                            (
                                Contents::Leaf { records, .. },
                                Contents::Leaf {
                                    records: sibling_records,
                                    ..
                                },
                            ) => {
                                if sibling_index < index {
                                    child.keys.insert(0, sibling.keys.pop().unwrap());
                                    records.insert(0, sibling_records.pop().unwrap());
                                    node.keys[separator_index] = child.keys[0];
                                } else {
                                    child.keys.push(sibling.keys.remove(0));
                                    records.push(sibling_records.remove(0));
                                    node.keys[separator_index] = sibling.keys[0];
                                }
                            }
                            (
                                Contents::Internal {
                                    children: child_children,
                                },
                                Contents::Internal {
                                    children: sibling_children,
                                },
                            ) => {
                                if sibling_index < index {
                                    child.keys.insert(0, node.keys[separator_index]);
                                    child_children.insert(0, sibling_children.pop().unwrap());
                                    node.keys[separator_index] = sibling.keys.pop().unwrap();
                                } else {
                                    child.keys.push(node.keys[separator_index]);
                                    child_children.push(sibling_children.remove(0));
                                    node.keys[separator_index] = sibling.keys.remove(0);
                                }
                            }
                            _ => {
                                return Err(invalid_data(
                                    "index siblings have different node types",
                                ));
                            }
                        }
                        self.write_node(children[index], &child)?;
                        self.write_node(children[sibling_index], &sibling)?;
                    } else {
                        let (mut left, right) = if sibling_index < index {
                            (sibling, child)
                        } else {
                            (child, sibling)
                        };
                        match (&mut left.contents, right.contents) {
                            (
                                Contents::Leaf { records, next },
                                Contents::Leaf {
                                    records: right_records,
                                    next: right_next,
                                },
                            ) => {
                                records.extend(right_records);
                                *next = right_next;
                            }
                            (
                                Contents::Internal {
                                    children: left_children,
                                },
                                Contents::Internal {
                                    children: right_children,
                                },
                            ) => {
                                left.keys.push(node.keys[separator_index]);
                                left_children.extend(right_children);
                            }
                            _ => {
                                return Err(invalid_data(
                                    "index siblings have different node types",
                                ));
                            }
                        }
                        left.keys.extend(right.keys);
                        self.write_node(children[separator_index], &left)?;
                        self.retire_node(children[separator_index + 1])?;
                        children.remove(separator_index + 1);
                        node.keys.remove(separator_index);
                    }
                }
            }
        }
        self.write_node(page_id, &node)?;
        Ok((record, new_minimum))
    }

    fn validate_subtree(
        &mut self,
        page_id: PageId,
        visited: &mut HashSet<PageId>,
        leaves: &mut Vec<(PageId, PageId)>,
        depth: usize,
    ) -> io::Result<(Option<i64>, Option<i64>, usize)> {
        if depth >= 64 || !visited.insert(page_id) {
            return Err(invalid_data(
                "index cycle, shared child, or excessive depth",
            ));
        }
        let node = self.read_node(page_id)?;
        if page_id != ROOT && node.keys.len() < node.capacity() / 2 {
            return Err(invalid_data("underfull non-root index node"));
        }
        match node.contents {
            Contents::Leaf { next, .. } => {
                leaves.push((page_id, next));
                Ok((node.keys.first().copied(), node.keys.last().copied(), 0))
            }
            Contents::Internal { children } => {
                if node.keys.is_empty() {
                    return Err(invalid_data("empty internal index root"));
                }
                let (minimum, mut maximum, height) =
                    self.validate_subtree(children[0], visited, leaves, depth + 1)?;
                for (index, &child) in children[1..].iter().enumerate() {
                    let (child_min, child_max, child_height) =
                        self.validate_subtree(child, visited, leaves, depth + 1)?;
                    if child_min != Some(node.keys[index])
                        || maximum >= child_min
                        || child_height != height
                    {
                        return Err(invalid_data(
                            "invalid index separator, child ordering, or leaf depth",
                        ));
                    }
                    maximum = child_max;
                }
                Ok((minimum, maximum, height + 1))
            }
        }
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(ErrorKind::InvalidData, message)
}
