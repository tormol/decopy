/* Copyright 2023 Torbjørn Birch Moltu
 *
 * This file is part of Decopy.
 * Decopy is free software: you can redistribute it and/or modify it under the
 * terms of the GNU General Public License as published by the Free Software Foundation,
 * either version 3 of the License, or (at your option) any later version.
 *
 * Decopy is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
 * without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
 * See the GNU General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along with Decopy.
 * If not, see <https://www.gnu.org/licenses/>.
 */

use std::collections::btree_map::{BTreeMap, Iter, Range};
use std::iter::Map;
use std::fmt::{Debug, Formatter, Result};
use std::ops::{RangeBounds, Bound};

/// A `BTreeMap` which allows multiple entries with the same key.
///
/// # Comparison with the [btreemultimap](https://crates.io/crates/btreemultimap) crate
///
/// btreemultimap uses a `BTreeMap<K, Vec<V>>` under the hood, while this crate
/// uses a `BTreeMap<(K, u32), V>`. This makes some different tradeoffs:
///
/// First off, chhanging btreemultimap to use a `SmallVec<T, 1>` would be
/// no-brainer except when V is very large and more than one will be common.
/// But assuming that optimization is made, it still allocates for every key
/// that has more than one value.
///
/// This crate avoids that by adding a hidden `u32` or `usize` to the key, and
/// using it as a counter.
/// This should reduce the number allocations, since per-key allocations are
/// replaced with extra b-tree nodes which should be more balanced.
/// The downside is that there will be multiple identical keys.
/// That doesn't matter if the key is cheap, like an integer, reference or
/// reference-counted, but will likely more than cancel out any benefit if the
/// key is individually allocated, such as `String` or `Vec`.
#[repr(transparent)]
pub struct BTreeMultiMap<K, V> {
    map: BTreeMap<(K, u32), V>,
}

impl<K, V> Default for BTreeMultiMap<K, V> {
    fn default() -> Self {
        BTreeMultiMap { map: BTreeMap::default() }
    }
}

pub struct BTreeMultiMapIter<'a, K, V, I=u32> {
    iter: Iter<'a, (K,I), V>,
}

const fn map_ref<'a, K, V, I>(((ref key, _), value): (&'a (K, I), &'a V)) -> (&'a K, &'a V) {
    (key, value)
}

type MapRef<'a, K, V, I> = Map<I, fn((&'a(K, u32), &'a V))->(&'a K, &'a V)>;

impl<'a, K, V, I> Iterator for BTreeMultiMapIter<'a, K, V, I> {
    type Item = (&'a K, &'a V);
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(map_ref)
    }
}

impl<'a, K, V> IntoIterator for &'a BTreeMultiMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = BTreeMultiMapIter<'a, K, V>;
    fn into_iter(self) -> Self::IntoIter {
        BTreeMultiMapIter { iter: self.map.iter() }
    }
}

impl<K: Debug, V: Debug> Debug for BTreeMultiMap<K, V> {
    fn fmt(&self,  fmtr: &mut Formatter) -> Result {
        fmtr.debug_map()
            .entries(self)
            .finish()
    }
}

impl<K, V> BTreeMultiMap<K, V> {
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }
}

impl<K:Ord, V> BTreeMultiMap<K, V> {
    pub fn first_key_value(&self) -> Option<(&K, &V)> {
        self.map.first_key_value().map(map_ref)
    }
    pub fn last_key_value(&self) -> Option<(&K, &V)> {
        self.map.last_key_value().map(map_ref)
    }
    pub fn range<R: RangeBounds<K>>(&self,  range: R) -> MapRef<K, V, Range<(K, u32), V>>
    where K: Clone {
        match (range.start_bound(), range.end_bound()) {
            (Bound::Unbounded, Bound::Unbounded) => self.map.range(..),
            (Bound::Unbounded, Bound::Excluded(end)) => self.map.range(..(end.clone(), 0u32)),
            (Bound::Unbounded, Bound::Included(end)) => self.map.range(..=(end.clone(), !0u32)),
            (Bound::Included(start), Bound::Unbounded) => self.map.range((start.clone(), 0u32)..),
            (Bound::Included(start), Bound::Excluded(end))
                => self.map.range((start.clone(), 0u32)..(end.clone(), 0u32)),
            (Bound::Included(start), Bound::Included(end))
                => self.map.range((start.clone(), 0u32)..=(end.clone(), !0u32)),
            (Bound::Excluded(start), Bound::Unbounded)
                => self.map.range((Bound::Excluded((start.clone(), !0u32)), Bound::Unbounded)),
            (Bound::Excluded(start), Bound::Excluded(end)) => self.map.range((
                Bound::Excluded((start.clone(), !0u32)),
                Bound::Excluded((end.clone(), 0u32)),
            )),
            (Bound::Excluded(start), Bound::Included(end)) => self.map.range((
                Bound::Excluded((start.clone(), !0u32)),
                Bound::Included((end.clone(), !0u32)),
            )),
        }.map(map_ref)
    }
    pub fn insert(&mut self,  key: K,  value: V) {
        let last_possible = (key, !0);
        let next_index = match self.map.range(..=&last_possible).last() {
            Some(((last, i), _)) if last == &last_possible.0 => *i + 1,
            _ => 0,
        };
        if let Some(_) = self.map.insert((last_possible.0, next_index), value) {
            unreachable!("There already is a value with index {}", next_index);
        }
    }
    pub fn remove_first(&mut self,  key: K) -> Option<V> {
        let key = (key, 0);
        match self.map.range(&key..).next() {
            Some(((first, i), _)) if first == &key.0 => self.map.remove(&(key.0, *i)),
            _ => None,
        }
    }
    pub fn remove_last(&mut self,  key: K) -> Option<V> {
        let key = (key, !0);
        match self.map.range(..=&key).last() {
            Some(((last, i), _)) if last == &key.0 => self.map.remove(&(key.0, *i)),
            _ => None,
        }
    }
}
