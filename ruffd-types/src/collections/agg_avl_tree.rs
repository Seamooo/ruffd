use crate::error::AggAvlTreeError;

type AggFn<T> = fn(&T, &T) -> T;

struct ChildNode<T> {
    /// Option for ease of swapping values without a default
    left: Option<Box<TreeNode<T>>>,

    /// Option for ease of swapping values without a default
    right: Option<Box<TreeNode<T>>>,

    /// Refers to the number of child nodes below this
    /// ie height excluding leaf nodes
    ///
    /// note child_height is i64 for easier diffs
    child_height: i64,
    elem_count: usize,
    agg: T,
}

impl<T> ChildNode<T>
where
    T: Clone,
{
    /// Requires both left and right nodes to be defined
    ///
    /// Use case for child node is to group 2 leaf nodes, or recursive children
    pub fn new(left: Box<TreeNode<T>>, right: Box<TreeNode<T>>, agg_fn: AggFn<T>) -> Self {
        let left = Some(left);
        let right = Some(right);
        let agg = Self::calc_agg(&left, &right, agg_fn);
        let mut rv = Self {
            left,
            right,
            child_height: 0,
            elem_count: 0,
            agg,
        };
        rv.update_node(agg_fn);
        rv
    }

    fn calc_agg(
        left: &Option<Box<TreeNode<T>>>,
        right: &Option<Box<TreeNode<T>>>,
        agg_fn: AggFn<T>,
    ) -> T {
        match left {
            Some(x) => {
                let x_agg = x.get_agg();
                match right {
                    Some(y) => agg_fn(x_agg, y.get_agg()),
                    None => x_agg.clone(),
                }
            }
            None => match right {
                Some(x) => x.get_agg().clone(),
                _ => panic!("child node by definition must have at least 1 child"),
            },
        }
    }

    fn update_agg(&mut self, agg_fn: AggFn<T>) {
        self.agg = Self::calc_agg(&self.left, &self.right, agg_fn);
    }

    fn update_height(&mut self) {
        let height = {
            let mut rv = self.get_left_height();
            if let Some(tp) = self.get_right_height() {
                rv = match rv {
                    None => Some(tp),
                    Some(val) => Some(val.max(tp)),
                };
            }
            rv
        };
        self.child_height = match height {
            None => 0,
            Some(x) => x + 1,
        };
    }

    fn update_elem_count(&mut self) {
        let left_count = if let Some(x) = &self.left {
            x.get_elem_count()
        } else {
            0
        };
        let right_count = if let Some(x) = &self.right {
            x.get_elem_count()
        } else {
            0
        };
        self.elem_count = left_count + right_count;
    }

    /// method for recomputing aggregate and height
    ///
    /// **Must** call this method on mutation of left or right
    /// values
    pub fn update_node(&mut self, agg_fn: AggFn<T>) {
        self.update_agg(agg_fn);
        self.update_height();
        self.update_elem_count();
    }

    fn get_left_height(&self) -> Option<i64> {
        match &self.left {
            None => None,
            Some(x) => x.get_height(),
        }
    }

    fn get_right_height(&self) -> Option<i64> {
        match &self.right {
            None => None,
            Some(x) => x.get_height(),
        }
    }

    fn get_left_elem_count(&self) -> usize {
        match &self.left {
            None => 0,
            Some(x) => x.get_elem_count(),
        }
    }
}

struct LeafNode<T> {
    val: T,
}

impl<T> LeafNode<T> {
    fn new(val: T) -> Self {
        Self { val }
    }
}

enum TreeNode<T> {
    Leaf(LeafNode<T>),
    Child(ChildNode<T>),
}

impl<T> TreeNode<T>
where
    T: Clone,
{
    pub fn get_range<R>(&self, range: R, agg_fn: AggFn<T>) -> Option<T>
    where
        R: std::ops::RangeBounds<usize>,
    {
        let start_idx = match range.start_bound() {
            std::ops::Bound::Included(x) => *x,
            std::ops::Bound::Excluded(x) => x + 1usize,
            std::ops::Bound::Unbounded => 0usize,
        };
        let end_idx = match range.end_bound() {
            std::ops::Bound::Included(x) => *x + 1usize,
            std::ops::Bound::Excluded(x) => *x,
            std::ops::Bound::Unbounded => self.get_elem_count() + 1,
        };
        match self {
            Self::Leaf(x) => {
                if start_idx > 0 || end_idx < 1 {
                    None
                } else {
                    Some(x.val.clone())
                }
            }
            Self::Child(x) => {
                // early stopping for entire tree segment
                if start_idx == 0 && end_idx >= x.elem_count {
                    return Some(x.agg.clone());
                }
                let mid_idx = x.get_left_elem_count();
                let lhs_end = mid_idx.min(end_idx);
                let lhs_result = if start_idx < mid_idx {
                    match &x.left {
                        Some(x) => x.get_range(start_idx..lhs_end, agg_fn),
                        None => None,
                    }
                } else {
                    None
                };
                let rhs_start = if start_idx > mid_idx {
                    start_idx - mid_idx
                } else {
                    0
                };
                let rhs_result = if mid_idx < end_idx {
                    match &x.right {
                        Some(x) => x.get_range(rhs_start..(end_idx - mid_idx), agg_fn),
                        None => None,
                    }
                } else {
                    None
                };
                match &lhs_result {
                    Some(x) => match &rhs_result {
                        Some(y) => Some(agg_fn(x, y)),
                        None => Some(x.clone()),
                    },
                    None => rhs_result,
                }
            }
        }
    }

    fn get_height(&self) -> Option<i64> {
        match self {
            Self::Leaf(_) => None,
            Self::Child(x) => Some(x.child_height),
        }
    }

    fn get_elem_count(&self) -> usize {
        match self {
            Self::Leaf(_) => 1,
            Self::Child(x) => x.elem_count,
        }
    }

    /// balances below case
    /// ```text
    ///  /
    /// /
    /// ```
    ///
    /// WARNING should only be reached via `self.balance`
    fn balance_ll(mut old_root: ChildNode<T>, agg_fn: AggFn<T>) -> Self {
        let mut rv = match *old_root.left.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        old_root.left = Some(rv.right.take().unwrap());
        old_root.update_node(agg_fn);
        rv.right = Some(Box::new(Self::Child(old_root)));
        rv.update_node(agg_fn);
        Self::Child(rv)
    }

    /// balances below case
    /// ```text
    /// /
    /// \
    /// ```
    ///
    /// WARNING should only be reached via `self.balance`
    fn balance_lr(mut old_root: ChildNode<T>, agg_fn: AggFn<T>) -> Self {
        let mut old_left = match *old_root.left.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        let mut ret_node = match *old_left.right.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        old_root.left = ret_node.right.take();
        old_root.update_node(agg_fn);
        old_left.right = ret_node.left.take();
        old_left.update_node(agg_fn);
        ret_node.left = Some(Box::new(Self::Child(old_left)));
        ret_node.right = Some(Box::new(Self::Child(old_root)));
        ret_node.update_node(agg_fn);
        Self::Child(ret_node)
    }

    /// balances below case
    /// ```text
    /// \
    /// /
    /// ```
    ///
    /// WARNING should only be reached via `self.balance`
    fn balance_rl(mut old_root: ChildNode<T>, agg_fn: AggFn<T>) -> Self {
        let mut old_right = match *old_root.right.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        let mut ret_node = match *old_right.left.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        old_root.right = ret_node.left.take();
        old_root.update_node(agg_fn);
        old_right.left = ret_node.right.take();
        old_right.update_node(agg_fn);
        ret_node.right = Some(Box::new(Self::Child(old_right)));
        ret_node.left = Some(Box::new(Self::Child(old_root)));
        ret_node.update_node(agg_fn);
        Self::Child(ret_node)
    }

    /// balances below case
    /// ```text
    /// \
    ///  \
    /// ```
    ///
    /// WARNING should only be reached via `self.balance`
    fn balance_rr(mut old_root: ChildNode<T>, agg_fn: AggFn<T>) -> Self {
        let mut rv = match *old_root.right.take().unwrap() {
            Self::Child(x) => x,
            _ => unreachable!(),
        };
        old_root.right = Some(rv.left.take().unwrap());
        old_root.update_node(agg_fn);
        rv.left = Some(Box::new(Self::Child(old_root)));
        rv.update_node(agg_fn);
        Self::Child(rv)
    }

    fn balance(self, agg_fn: AggFn<T>) -> Self {
        let node = match self {
            Self::Child(node) => node,
            Self::Leaf(node) => return Self::Leaf(node),
        };
        let left = node.left.as_ref().unwrap();
        let right = node.right.as_ref().unwrap();
        let left_height = left.get_height().unwrap_or(-1);
        let right_height = right.get_height().unwrap_or(-1);
        if (left_height - right_height).abs() < 2 {
            Self::Child(node)
        } else if left_height > right_height {
            let left_node = match *(*left) {
                Self::Child(ref node) => node,
                _ => unreachable!(),
            };
            let ll_height = left_node.get_left_height().unwrap_or(-1);
            let lr_height = left_node.get_right_height().unwrap_or(-1);
            if ll_height > lr_height {
                Self::balance_ll(node, agg_fn)
            } else {
                Self::balance_lr(node, agg_fn)
            }
        } else {
            let right_node = match *(*right) {
                Self::Child(ref node) => node,
                _ => unreachable!(),
            };
            let rl_height = right_node.get_left_height().unwrap_or(-1);
            let rr_height = right_node.get_right_height().unwrap_or(-1);
            if rl_height > rr_height {
                Self::balance_rl(node, agg_fn)
            } else {
                Self::balance_rr(node, agg_fn)
            }
        }
    }

    fn get_agg(&self) -> &T {
        match self {
            Self::Child(x) => &x.agg,
            Self::Leaf(x) => &x.val,
        }
    }

    pub fn insert(self, idx: usize, val: T, agg_fn: AggFn<T>) -> Self {
        let rv = match self {
            Self::Leaf(x) => {
                let tp_node = Box::new(Self::Leaf(LeafNode::new(val)));
                let curr_node = Box::new(Self::Leaf(x));
                let (left, right) = if idx == 0 {
                    (tp_node, curr_node)
                } else {
                    (curr_node, tp_node)
                };
                Self::Child(ChildNode::new(left, right, agg_fn))
            }
            Self::Child(mut x) => {
                let left_nelems = x.get_left_elem_count();
                let (left, right) = if idx > left_nelems {
                    let left_node = x.left.take().unwrap();
                    let right_node = Box::new(x.right.take().unwrap().insert(
                        idx - left_nelems,
                        val,
                        agg_fn,
                    ));
                    (left_node, right_node)
                } else {
                    let left_node = Box::new(x.left.take().unwrap().insert(idx, val, agg_fn));
                    let right_node = x.right.take().unwrap();
                    (left_node, right_node)
                };
                Self::Child(ChildNode::new(left, right, agg_fn))
            }
        };
        rv.balance(agg_fn)
    }

    pub fn update(&mut self, idx: usize, val: T, agg_fn: AggFn<T>) -> Result<(), AggAvlTreeError> {
        match self {
            Self::Child(x) => {
                let mid_idx = x.get_left_elem_count();
                let rv = if idx < mid_idx {
                    x.left.as_mut().unwrap().update(idx, val, agg_fn)
                } else {
                    x.right.as_mut().unwrap().update(idx - mid_idx, val, agg_fn)
                };
                if rv.is_ok() {
                    x.update_agg(agg_fn);
                }
                rv
            }
            Self::Leaf(x) => {
                if idx == 0 {
                    x.val = val;
                    Ok(())
                } else {
                    Err(AggAvlTreeError::IndexOutOfBounds)
                }
            }
        }
    }

    /// Delete index relative to tree node
    ///
    /// Panics if index out of bounds as short circuiting this can break
    /// the structure
    pub fn delete(self, idx: usize, agg_fn: AggFn<T>) -> Option<Self> {
        match self {
            Self::Child(x) => {
                let mid_idx = x.get_left_elem_count();
                let rv = if idx < mid_idx {
                    match x.left.unwrap().delete(idx, agg_fn) {
                        Some(y) => {
                            Self::Child(ChildNode::new(Box::new(y), x.right.unwrap(), agg_fn))
                        }
                        None => *x.right.unwrap(),
                    }
                } else {
                    match x.right.unwrap().delete(idx - mid_idx, agg_fn) {
                        Some(y) => {
                            Self::Child(ChildNode::new(x.left.unwrap(), Box::new(y), agg_fn))
                        }
                        None => *x.left.unwrap(),
                    }
                };
                Some(rv.balance(agg_fn))
            }
            Self::Leaf(_) => {
                if idx == 0 {
                    None
                } else {
                    panic!("attempted out of bounds delete");
                }
            }
        }
    }
}

/// AvlTree to enable a dynamic structure for fast
/// range aggregates
///
/// Insert, update, delete are all O(log_2(n))
///
/// query(range) is also O(log_2(n))
///
/// use `from_vec` for linear time construction, otherwise
/// inserting each node leads to O(n*log_2(n)) insertion
pub struct AggAvlTree<T> {
    root: Option<TreeNode<T>>,
    accumulate: AggFn<T>,
}

impl<T> AggAvlTree<T>
where
    T: Clone,
{
    pub fn new(accumulate: AggFn<T>) -> Self {
        Self {
            root: None,
            accumulate,
        }
    }

    pub fn from_vec(elems: Vec<T>, accumulate: fn(&T, &T) -> T) -> Self {
        // TODO build bottom up balanced bst inplace
        let mut rv = Self::new(accumulate);
        elems.into_iter().for_each(|x| rv.insert_back(x));
        rv
    }

    pub fn get(&self, idx: usize) -> Option<T> {
        self.get_range(idx..=idx)
    }

    /// retrieves aggregate across range specified
    ///
    /// returns `None` if there is no overlap between the specified
    /// range and the range of indexes present in the tree
    pub fn get_range<R>(&self, range: R) -> Option<T>
    where
        R: std::ops::RangeBounds<usize>,
    {
        match &self.root {
            Some(root) => root.get_range(range, self.accumulate),
            None => None,
        }
    }

    /// insert an element at the specified index
    ///
    /// if the index is larger than the element count, insert at the back
    pub fn insert(&mut self, idx: usize, val: T) {
        if let Some(root) = self.root.take() {
            self.root = Some(root.insert(idx, val, self.accumulate));
        } else {
            self.root = Some(TreeNode::Leaf(LeafNode::new(val)));
        }
    }

    pub fn insert_back(&mut self, val: T) {
        let idx = match &self.root {
            None => 0,
            Some(x) => x.get_elem_count(),
        };
        self.insert(idx + 1, val);
    }

    pub fn insert_front(&mut self, val: T) {
        self.insert(0, val);
    }

    pub fn update(&mut self, idx: usize, val: T) -> Result<(), AggAvlTreeError> {
        match &mut self.root {
            Some(x) => x.update(idx, val, self.accumulate),
            None => Err(AggAvlTreeError::IndexOutOfBounds),
        }
    }

    pub fn delete(&mut self, idx: usize) -> Result<(), AggAvlTreeError> {
        let result = match &self.root {
            Some(x) => {
                if idx >= x.get_elem_count() {
                    Err(AggAvlTreeError::IndexOutOfBounds)
                } else {
                    Ok(())
                }
            }
            None => Err(AggAvlTreeError::IndexOutOfBounds),
        };
        if result.is_ok() {
            self.root = match self.root.take() {
                Some(x) => x.delete(idx, self.accumulate),
                None => None,
            };
        }
        result
    }

    pub fn is_empty(&self) -> bool {
        self.root.is_none()
    }

    pub fn len(&self) -> usize {
        match &self.root {
            None => 0,
            Some(x) => x.get_elem_count(),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    fn agg_add<T>(a: &T, b: &T) -> T
    where
        T: std::ops::Add<Output = T> + Clone,
    {
        a.clone() + b.clone()
    }

    #[test]
    fn test_build() {
        let nums = (0..100).into_iter().collect::<Vec<_>>();
        AggAvlTree::from_vec(nums, agg_add);
    }

    #[test]
    fn test_aggregate() {
        let nums = (0..100).into_iter().collect::<Vec<_>>();
        let tree = AggAvlTree::from_vec(nums, agg_add);
        let t0_range = 2..5usize;
        let t1_range = 40..50usize;
        let t2_range = 0..100usize;
        let t3_range = 50..40usize;
        let ranges = vec![t0_range, t1_range, t2_range, t3_range];
        ranges.into_iter().for_each(|x| {
            let expected = x.clone().into_iter().reduce(|a, b| agg_add(&a, &b));
            let result = tree.get_range(x.clone());
            assert_eq!(result, expected, "failed on range: {:?}", x);
        });
    }

    #[test]
    fn test_update() {
        let nums = (0..100).into_iter().collect::<Vec<_>>();
        let mut tree = AggAvlTree::from_vec(nums, agg_add);
        tree.update(4, 6).unwrap();
        let result = tree.get_range(2..5).unwrap();
        assert_eq!(result, 9 + 2);
    }

    #[test]
    fn test_delete() {
        let nums = (0..100).into_iter().collect::<Vec<_>>();
        let mut tree = AggAvlTree::from_vec(nums, agg_add);
        tree.delete(3).unwrap();
        let result = tree.get_range(2..4).unwrap();
        assert_eq!(result, 9 - 3);
    }
}
