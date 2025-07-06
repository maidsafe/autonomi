// Copyright 2024 MaidSafe.net limited.
//
// This SAFE Network Software is licensed to you under The General Public License (GPL), version 3.
// Unless required by applicable law or agreed to in writing, the SAFE Network Software distributed
// under the GPL Licence is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
// KIND, either express or implied. Please review the Licences for the specific language governing
// permissions and limitations relating to use of the SAFE Network Software.

/// Based on https://users.rust-lang.org/t/the-best-ring-buffer-library/58489/7
///
/// A circular buffer implemented with a VecDeque.
#[derive(Debug)]
pub(crate) struct CircularVec<T> {
    inner: std::collections::VecDeque<T>,
}

impl<T> CircularVec<T> {
    /// Creates a new CircularVec with the given capacity.
    ///
    /// Capacity is normally rounded up to the nearest power of 2, minus one. E.g. 15, 31, 63, 127, 255, etc.
    pub(crate) fn new(capacity: usize) -> Self {
        Self {
            inner: std::collections::VecDeque::with_capacity(capacity),
        }
    }

    /// Pushes an item into the CircularVec. If the CircularVec is full, the oldest item is removed.
    pub(crate) fn push(&mut self, item: T) {
        if self.inner.len() == self.inner.capacity() {
            let _ = self.inner.pop_front();
        }
        self.inner.push_back(item);
    }

    /// Checks if the CircularVec contains the given item.
    pub(crate) fn contains(&self, item: &T) -> bool
    where
        T: PartialEq,
    {
        self.inner.contains(item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_and_contains() {
        let mut cv = CircularVec::new(2);
        cv.push(1);
        cv.push(2);
        assert!(cv.contains(&1));
        assert!(cv.contains(&2));

        cv.push(3);
        assert!(!cv.contains(&1));
        assert!(cv.contains(&2));
        assert!(cv.contains(&3));

        assert!(cv.inner.len() == 2);
    }
}
