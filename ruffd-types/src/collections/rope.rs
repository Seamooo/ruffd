use crate::error::RopeError;
use std::collections::VecDeque;
use std::fmt;
use std::ops::{Bound, RangeBounds};

// NOTE There's a lot of room for better memory management in this collection
// implementation, however, everything exists without unsafe blocks for now,
// which is nice

// TODO make LEAF_SIZE size compilation configurable, and type dependant, such that
// a leaf will always fit in a cache line, (will come when val is a sized slice rather)
// than a vector
const LEAF_SIZE: usize = 64;

#[derive(Debug)]
enum Lr<T> {
    Left(T),
    Right(T),
}

impl<T> Lr<T> {
    fn new(val: T, is_left: bool) -> Self {
        if is_left {
            Self::Left(val)
        } else {
            Self::Right(val)
        }
    }
}

struct L2Val<T> {
    parent: Box<RopeParent<T>>,
    target: Lr<Box<RopeParent<T>>>,
}

impl<T> L2Val<T> {
    fn new(parent: Box<RopeParent<T>>, target: Lr<Box<RopeParent<T>>>) -> Self {
        Self { parent, target }
    }
}

enum SplayRet<T> {
    L1(Box<RopeParent<T>>),
    L2(L2Val<T>),
    Leaf(Vec<T>),
}

impl<T> From<RopeNode<T>> for SplayRet<T> {
    fn from(node: RopeNode<T>) -> Self {
        match node {
            RopeNode::Parent(x) => Self::L1(x),
            RopeNode::Leaf(x) => Self::Leaf(x),
        }
    }
}

impl<T> From<SplayRet<T>> for RopeNode<T> {
    fn from(splay_ret: SplayRet<T>) -> Self {
        match splay_ret {
            SplayRet::L1(x) => Self::Parent(x),
            SplayRet::L2(L2Val { parent, target }) => Self::zig_splay(*parent, target),
            SplayRet::Leaf(x) => Self::Leaf(x),
        }
    }
}

enum RopeNode<T> {
    Leaf(Vec<T>),
    Parent(Box<RopeParent<T>>),
}

impl<T> fmt::Debug for RopeNode<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Leaf(x) => f.debug_tuple("Leaf").field(&x.len()).finish(),
            Self::Parent(x) => f.debug_tuple("Parent").field(&x).finish(),
        }
    }
}

struct RopeParent<T> {
    // internal values are only option to enable swap with
    // no default
    left: Option<RopeNode<T>>,
    right: Option<RopeNode<T>>,
    elem_count: usize,
}

impl<T> fmt::Debug for RopeParent<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RopeParent")
            .field("left", &self.left)
            .field("right", &self.right)
            .field("elem_count", &self.elem_count)
            .finish()
    }
}

impl<T> RopeParent<T> {
    fn new(lhs: RopeNode<T>, rhs: RopeNode<T>) -> Self {
        let left = Some(lhs);
        let right = Some(rhs);
        let mut rv = Self {
            left,
            right,
            elem_count: 0,
        };
        rv.update_node();
        rv
    }

    fn get_left_elem_count(&self) -> usize {
        match &self.left {
            None => 0,
            Some(x) => x.elem_count(),
        }
    }

    fn update_elem_count(&mut self) {
        let left_count = if let Some(x) = &self.left {
            x.elem_count()
        } else {
            0
        };
        let right_count = if let Some(x) = &self.right {
            x.elem_count()
        } else {
            0
        };
        self.elem_count = left_count + right_count;
    }

    /// Method for recomputing elem_count
    ///
    /// **Must** call this method on mutation of left or right
    /// values
    pub fn update_node(&mut self) {
        self.update_elem_count();
    }
}

impl<T> RopeNode<T> {
    pub fn new(mut val: Vec<T>) -> Self {
        if val.len() > LEAF_SIZE {
            let mid_idx = val.len() >> 1;
            let rhs = val.drain(mid_idx..).collect::<Vec<_>>();
            let rhs_node = Self::new(rhs);
            let lhs_node = Self::new(val);
            Self::Parent(Box::new(RopeParent::new(lhs_node, rhs_node)))
        } else {
            Self::Leaf(val)
        }
    }

    pub fn from_nodes(lhs: Self, rhs: Self) -> Self {
        if lhs.elem_count() + rhs.elem_count() < LEAF_SIZE {
            let mut val = lhs.drain();
            let mut tp = rhs.drain();
            val.append(&mut tp);
            Self::Leaf(val)
        } else {
            Self::Parent(Box::new(RopeParent::new(lhs, rhs)))
        }
    }

    pub fn elem_count(&self) -> usize {
        match self {
            Self::Parent(x) => x.elem_count,
            Self::Leaf(x) => x.len(),
        }
    }

    fn drain(self) -> Vec<T> {
        match self {
            Self::Leaf(x) => x,
            Self::Parent(x) => {
                let mut rv = x.left.unwrap().drain();
                let mut rhs = x.right.unwrap().drain();
                rv.append(&mut rhs);
                rv
            }
        }
    }

    fn splay(
        grandparent: RopeParent<T>,
        parent: Lr<Box<RopeParent<T>>>,
        target: Lr<Box<RopeParent<T>>>,
    ) -> Self {
        // NOTE this method assumes that self and parent have removed parent
        // and target from the corresponding left and right fields
        match parent {
            Lr::Left(mut parent_node) => match target {
                Lr::Left(mut target_node) => {
                    let new_grandparent = Self::from_nodes(
                        parent_node.right.take().unwrap(),
                        grandparent.right.unwrap(),
                    );
                    let new_parent =
                        Self::from_nodes(target_node.right.take().unwrap(), new_grandparent);
                    Self::from_nodes(target_node.left.take().unwrap(), new_parent)
                }
                Lr::Right(mut target_node) => {
                    let new_grandparent = Self::from_nodes(
                        target_node.right.take().unwrap(),
                        grandparent.right.unwrap(),
                    );
                    let new_parent =
                        Self::from_nodes(parent_node.left.unwrap(), target_node.left.unwrap());
                    Self::from_nodes(new_parent, new_grandparent)
                }
            },
            Lr::Right(mut parent_node) => match target {
                Lr::Left(mut target_node) => {
                    let new_grandparent = Self::from_nodes(
                        grandparent.left.unwrap(),
                        target_node.left.take().unwrap(),
                    );
                    let new_parent = Self::from_nodes(
                        target_node.right.take().unwrap(),
                        parent_node.right.unwrap(),
                    );
                    Self::from_nodes(new_grandparent, new_parent)
                }
                Lr::Right(mut target_node) => {
                    let new_grandparent = Self::from_nodes(
                        grandparent.left.unwrap(),
                        parent_node.left.take().unwrap(),
                    );
                    let new_parent =
                        Self::from_nodes(new_grandparent, target_node.left.take().unwrap());
                    Self::from_nodes(new_parent, target_node.right.take().unwrap())
                }
            },
        }
    }

    fn zig_splay(parent: RopeParent<T>, target: Lr<Box<RopeParent<T>>>) -> Self {
        match target {
            Lr::Left(mut target_node) => {
                let new_parent =
                    Self::from_nodes(target_node.right.take().unwrap(), parent.right.unwrap());
                Self::from_nodes(target_node.left.unwrap(), new_parent)
            }
            Lr::Right(mut target_node) => {
                let new_parent =
                    Self::from_nodes(parent.left.unwrap(), target_node.left.take().unwrap());
                Self::from_nodes(new_parent, target_node.right.unwrap())
            }
        }
    }

    /// Insert a vector into the rope at the given index
    ///
    /// If the provided index is greater than the maximum,
    /// the value will be inserted at the back
    pub fn insert(self, mut val: Vec<T>, idx: usize) -> SplayRet<T> {
        match self {
            Self::Leaf(mut x) => {
                let mut rhs = x.drain(idx..).collect::<Vec<_>>();
                x.append(&mut val);
                x.append(&mut rhs);
                Self::new(x).into()
            }
            Self::Parent(mut parent_node) => {
                let mid_idx = parent_node.get_left_elem_count();
                let (is_left, ret_val) = if idx < mid_idx {
                    // take such that no move occurs
                    let rv = parent_node.left.take().unwrap().insert(val, idx);
                    (true, rv)
                } else {
                    let rv = parent_node.right.take().unwrap().insert(val, idx - mid_idx);
                    (false, rv)
                };
                match ret_val {
                    SplayRet::L1(x) => SplayRet::L2(L2Val::new(parent_node, Lr::new(x, is_left))),
                    SplayRet::L2(L2Val { parent, target }) => {
                        Self::splay(*parent_node, Lr::new(parent, is_left), target).into()
                    }
                    SplayRet::Leaf(x) => {
                        let ret_node = if is_left {
                            Self::from_nodes(Self::new(x), parent_node.right.unwrap())
                        } else {
                            Self::from_nodes(parent_node.left.unwrap(), Self::new(x))
                        };
                        ret_node.into()
                    }
                }
            }
        }
    }

    pub fn delete<R: RangeBounds<usize>>(self, range: R) -> Option<Self> {
        match self {
            Self::Leaf(mut val) => {
                val.drain(range).for_each(drop);
                if val.is_empty() {
                    None
                } else {
                    Some(Self::new(val))
                }
            }
            Self::Parent(mut node) => {
                let start_idx = match range.start_bound() {
                    Bound::Included(x) => *x,
                    Bound::Excluded(x) => x + 1usize,
                    Bound::Unbounded => 0usize,
                };
                let end_idx = match range.end_bound() {
                    Bound::Included(x) => *x + 1usize,
                    Bound::Excluded(x) => *x,
                    Bound::Unbounded => node.elem_count,
                };
                let mid_idx = node.get_left_elem_count();
                let left = node.left.take().unwrap();
                let lhs = if start_idx < mid_idx {
                    let end_bound = mid_idx.min(end_idx);
                    left.delete(start_idx..end_bound)
                } else {
                    Some(left)
                };
                let right = node.right.take().unwrap();
                let rhs = if end_idx > mid_idx {
                    let start_bound = start_idx.max(mid_idx) - mid_idx;
                    right.delete(start_bound..(end_idx - mid_idx))
                } else {
                    Some(right)
                };
                match (lhs, rhs) {
                    (None, rhs) => rhs,
                    (lhs, None) => lhs,
                    (Some(lhs), Some(rhs)) => Some(Self::from_nodes(lhs, rhs)),
                }
            }
        }
    }
}

pub struct RopeIterator<'a, T> {
    /// Call stack for dfs
    node_stack: VecDeque<&'a RopeParent<T>>,

    /// Number of iteration calls expected if Some else infinite
    iter_len: Option<usize>,

    /// Current internal iteration index
    curr_idx: usize,

    /// Current item iterator
    item_iter: Box<dyn Iterator<Item = &'a T> + 'a>,
}

impl<'a, T> RopeIterator<'a, T> {
    fn new<R: RangeBounds<usize>>(root: &'a RopeNode<T>, range: R) -> Self {
        let mut curr_node = root;
        let mut node_stack = VecDeque::<&'a RopeParent<T>>::new();
        let start_idx = match range.start_bound() {
            Bound::Included(x) => *x,
            Bound::Excluded(x) => x + 1usize,
            Bound::Unbounded => 0,
        };
        let iter_len = match range.end_bound() {
            Bound::Included(x) => Some(x - start_idx + 1),
            Bound::Excluded(x) => Some(x - start_idx),
            Bound::Unbounded => None,
        };
        let mut tp_agg = 0usize;
        while let RopeNode::Parent(node) = curr_node {
            let left_elem_count = node.left.as_ref().unwrap().elem_count();
            if start_idx - tp_agg >= left_elem_count {
                tp_agg += left_elem_count;
                curr_node = node.right.as_ref().unwrap();
            } else {
                node_stack.push_back(node);
                curr_node = node.left.as_ref().unwrap();
            }
        }
        let item_iter = Box::new(match curr_node {
            RopeNode::Leaf(x) => x.iter().skip(start_idx - tp_agg),
            _ => unreachable!(),
        });
        Self {
            node_stack,
            iter_len,
            curr_idx: 0,
            item_iter,
        }
    }

    fn empty() -> Self {
        Self {
            node_stack: VecDeque::<&'a RopeParent<T>>::new(),
            iter_len: None,
            curr_idx: 0,
            item_iter: Box::new(std::iter::empty()),
        }
    }
}

impl<'a, T> Iterator for RopeIterator<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(x) = self.iter_len {
            if x == self.curr_idx {
                return None;
            }
        }
        if let Some(x) = self.item_iter.next() {
            self.curr_idx += 1;
            return Some(x);
        }
        if let Some(x) = self.node_stack.pop_back() {
            let mut curr_node: &RopeNode<T> = x.right.as_ref().unwrap();
            while let RopeNode::Parent(x) = curr_node {
                self.node_stack.push_back(x);
                curr_node = x.left.as_ref().unwrap();
            }
            self.item_iter = match curr_node {
                RopeNode::Leaf(ref x) => Box::new(x.iter()),
                _ => unreachable!(),
            };
        } else {
            return None;
        }
        self.next()
    }
}

/// Rope datastructure for fast insert / delete ops
///
/// Novelty of this implementation is it performs a splay op after
/// each mutation op, such that traversal to similar indices
/// is dynamically optimal (unproven but Levy is nearly there!)
#[derive(Debug)]
pub struct Rope<T> {
    root: Option<RopeNode<T>>,
}

impl<T> Default for Rope<T> {
    fn default() -> Self {
        Self { root: None }
    }
}

impl<T> Rope<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_document(document: Vec<T>) -> Self {
        let root = Some(RopeNode::new(document));
        Self { root }
    }

    pub fn len(&self) -> usize {
        match &self.root {
            Some(x) => x.elem_count(),
            None => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    /// Inserts collection into the datastructure at the given index
    pub fn insert(&mut self, string: Vec<T>, idx: usize) -> Result<(), RopeError> {
        // use idx == self.len() for insert_back
        if idx > self.len() {
            return Err(RopeError::IndexOutOfBounds);
        }
        self.root = match self.root.take() {
            None => Some(RopeNode::new(string)),
            Some(x) => Some(x.insert(string, idx).into()),
        };
        Ok(())
    }

    pub fn delete<R: RangeBounds<usize>>(&mut self, range: R) {
        self.root = match self.root.take() {
            None => None,
            Some(x) => x.delete(range),
        };
    }

    pub fn iter(&self) -> RopeIterator<'_, T> {
        self.iter_range(..)
    }

    pub fn iter_range<R: RangeBounds<usize>>(&self, bounds: R) -> RopeIterator<'_, T> {
        match self.root {
            Some(ref x) => RopeIterator::new(x, bounds),
            None => RopeIterator::empty(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    const SMALL_PROGRAM: &str = r#"
def main():
    print('a small program')

if __name__ == '__main__':
    main()
"#;
    const SMALL_STR: &str = "0123456789";

    #[test]
    fn small_example() {
        let characters = SMALL_PROGRAM.chars().collect::<Vec<_>>();
        let rope = Rope::from_document(characters);
        let full_str = rope.iter().collect::<String>();
        assert_eq!(SMALL_PROGRAM, full_str.as_str());
    }

    #[test]
    fn small_case() {
        let characters = SMALL_STR.chars().collect::<Vec<_>>();
        let rope = Rope::from_document(characters);
        let full_str = rope.iter().collect::<String>();
        assert_eq!(SMALL_STR, full_str.as_str());
    }

    #[test]
    fn larger_case() {
        let characters = SMALL_STR.chars().cycle().take(100).collect::<Vec<_>>();
        let start_str = characters.iter().collect::<String>();
        let rope = Rope::from_document(characters);
        let result_str = rope.iter().collect::<String>();
        assert_eq!(start_str, result_str);
    }

    #[test]
    fn large_case() {
        let characters = SMALL_STR.chars().cycle().take(10000).collect::<Vec<_>>();
        let start_str = characters.iter().collect::<String>();
        let rope = Rope::from_document(characters);
        let result_str = rope.iter().collect::<String>();
        assert_eq!(start_str, result_str);
    }

    #[test]
    fn insert_back() {
        let characters = SMALL_PROGRAM.chars().collect::<Vec<_>>();
        let mut rope = Rope::from_document(characters);
        rope.insert(String::from("some text").chars().collect::<Vec<_>>(), 81)
            .unwrap();
        let full_str = rope.iter().collect::<String>();
        let expected = r#"
def main():
    print('a small program')

if __name__ == '__main__':
    main()
some text"#;
        assert_eq!(full_str, expected);
    }

    #[test]
    fn delete_case() {
        let characters = SMALL_PROGRAM.chars().collect::<Vec<_>>();
        let mut rope = Rope::from_document(characters);
        rope.delete(5..9);
        let full_str = rope.iter().collect::<String>();
        let expected = r#"
def ():
    print('a small program')

if __name__ == '__main__':
    main()
"#;
        assert_eq!(full_str, expected);
    }

    #[test]
    fn consecutive_updates() {
        let characters = SMALL_PROGRAM.chars().collect::<Vec<_>>();
        let mut rope = Rope::from_document(characters);
        rope.delete(14..14);
        rope.insert("    x = 3".chars().collect::<Vec<_>>(), 14)
            .unwrap();
        rope.delete(14..23);
        rope.insert(vec![], 14).unwrap();
        let full_str = rope.iter().collect::<String>();
        assert_eq!(full_str, SMALL_PROGRAM);
    }
}
