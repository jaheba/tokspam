//! Bounded Vec-backed segmented stacks; annotation scans share buffers without copying entries.
use std::ops::{Deref, DerefMut};

pub(crate) const SCAN_FRAMES: usize = crate::MAX_ANNOTATION_DEPTH as usize + 1;
// Roots for all live scans plus one entry observed before structural rejection.
pub(crate) const STRUCTURAL_CAPACITY: usize = crate::MAX_NESTING_DEPTH as usize + SCAN_FRAMES + 1;

#[derive(Clone, Copy, Default)]
pub(crate) struct Mark {
    base: u16,
    len: u16,
}

pub(crate) struct Stack<T, const N: usize> {
    data: Vec<T>,
    base: usize,
    capacity_limit: usize,
}

impl<T, const N: usize> Default for Stack<T, N> {
    fn default() -> Self {
        Self::with_storage(N, false)
    }
}

impl<T, const N: usize> Stack<T, N> {
    pub(crate) fn with_storage(capacity: usize, preallocated: bool) -> Self {
        const {
            assert!(N <= u16::MAX as usize);
        }
        assert!(capacity <= N);

        Self {
            data: if preallocated {
                Vec::with_capacity(capacity)
            } else {
                Vec::new()
            },
            base: 0,
            capacity_limit: capacity,
        }
    }

    pub(crate) fn reset(&mut self) {
        self.data.clear();
        self.base = 0;
    }

    pub(crate) fn clear(&mut self) {
        self.data.truncate(self.base);
    }

    #[cfg(feature = "regions")]
    pub(crate) fn truncate(&mut self, length: usize) {
        self.data.truncate(self.base + length);
    }

    pub(crate) fn is_full(&self) -> bool {
        self.data.len() == N
    }

    pub(crate) fn push(&mut self, value: T) {
        assert!(self.data.len() < self.capacity_limit && !self.is_full());
        if self.data.len() == self.data.capacity() {
            self.grow();
        }
        self.data.push(value);
    }

    #[cold]
    fn grow(&mut self) {
        // Vec's automatic doubling may exceed the configured ceiling. Reserve
        // explicitly so growing and preallocated buffers share the same bound.
        // Suspended annotations store indices, not element pointers, so moving
        // the backing allocation cannot invalidate their segment marks.
        let capacity = self
            .data
            .capacity()
            .saturating_mul(2)
            .max(4)
            .min(self.capacity_limit);
        self.data.reserve_exact(capacity - self.data.len());
    }

    pub(crate) fn suspend(&mut self) -> Mark {
        let mark = Mark {
            base: self.base as u16,
            len: self.data.len() as u16,
        };
        self.base = self.data.len();
        mark
    }

    pub(crate) fn restore(&mut self, mark: Mark) {
        self.base = usize::from(mark.base);
        self.data.truncate(usize::from(mark.len));
    }
}

impl<T, const N: usize> Stack<T, N> {
    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.data.len() == self.base {
            None
        } else {
            self.data.pop()
        }
    }
}

impl<T, const N: usize> Deref for Stack<T, N> {
    type Target = [T];

    fn deref(&self) -> &[T] {
        &self.data[self.base..]
    }
}

impl<T, const N: usize> DerefMut for Stack<T, N> {
    fn deref_mut(&mut self) -> &mut [T] {
        &mut self.data[self.base..]
    }
}

#[cfg(test)]
mod tests {
    use super::Stack;

    #[test]
    fn growth_stays_within_the_configured_ceiling_and_reset_retains_capacity() {
        let mut stack = Stack::<u32, 32>::with_storage(19, false);
        assert_eq!(stack.data.capacity(), 0);
        for value in 0..19 {
            stack.push(value);
            assert!(stack.data.capacity() <= 19);
        }
        assert_eq!(stack.data.capacity(), 19);
        stack.reset();
        assert!(stack.is_empty());
        assert_eq!(stack.data.capacity(), 19);
        for value in 0..19 {
            stack.push(value);
        }
        assert_eq!(stack.data.capacity(), 19);
    }

    #[test]
    fn nested_segments_restore_parents_and_isolate_roots() {
        let mut stack = Stack::<u32, 4>::default();
        stack.push(1);
        let parent = stack.suspend();
        assert!(stack.is_empty());
        assert_eq!(stack.pop(), None);
        stack.push(2);
        let child = stack.suspend();
        stack.push(3);
        stack.push(4);
        assert_eq!(&*stack, &[3, 4]);
        stack.clear();
        assert_eq!(stack.pop(), None);
        stack.restore(child);
        assert_eq!(&*stack, &[2]);
        stack.restore(parent);
        assert_eq!(&*stack, &[1]);

        // A failed scan may abandon a child segment. Reset discards all bases.
        stack.suspend();
        stack.push(5);
        stack.reset();
        assert!(stack.is_empty());
        stack.push(6);
        assert_eq!(&*stack, &[6]);
    }
}
