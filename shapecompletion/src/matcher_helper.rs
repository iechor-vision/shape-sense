use std::{
    collections::HashMap,
    hash::Hash,
    ops::{Index, IndexMut},
    slice::Iter,
    vec::IntoIter,
};

use visioniechor::PointF64;

#[derive(Clone, Copy, Debug)]
pub struct MatchItem {
    pub id: usize,
    pub point: PointF64,
    pub direction: PointF64,
}

#[derive(Debug, Default)]
pub struct MatchItemSet {
    pub items: Vec<MatchItem>,
}

#[derive(Debug, Default)]
pub struct Matching {
    pub index_pairs: Vec<(usize, usize)>,
}

pub trait Distanced {
    fn distance_to(&self, other: &Self) -> f64;
}

/// A square matrix storing the pairwise distances of match items between 2 sets
pub struct SquareDistanceMatrix {
    pub n: usize,
    pub distances: Vec<f64>, // row-major
}

impl Distanced for MatchItem {
    fn distance_to(&self, other: &Self) -> f64 {
        self.point.distance_to(other.point)
    }
}

impl MatchItem {
    /// Create a MatchItem with a default id and the specified 'point' and 'direction'.
    pub fn new_with_default_id(point: PointF64, direction: PointF64) -> Self {
        Self {
            id: Default::default(),
            point,
            direction,
        }
    }
}

impl Index<usize> for MatchItemSet {
    type Output = MatchItem; // Output a row for further indexing

    fn index(&self, index: usize) -> &Self::Output {
        &self.items[index]
    }
}

impl MatchItemSet {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the id's of the items to ensure uniqueness
    pub fn from_match_items_and_set_ids(mut items: Vec<MatchItem>) -> Self {
        items.iter_mut().enumerate().for_each(|(i, item)| {
            item.id = i;
        });
        Self { items }
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn iter(&self) -> Iter<MatchItem> {
        self.items.iter()
    }

    pub fn remove(&mut self, index: usize) -> MatchItem {
        self.items.remove(index)
    }

    /// Ensure uniqueness of items
    /// Undefined behaviors may be caused if 'push_as_is' was called on this set.
    pub fn push_and_set_id(&mut self, mut match_item: MatchItem) {
        match_item.id = self.len();
        self.items.push(match_item);
    }

    /// Preserve id of items
    /// Undefined behaviors may be caused if 'push_and_set_id' was called on this set.
    pub fn push_as_is(&mut self, match_item: MatchItem) {
        self.items.push(match_item)
    }
}

fn sort_index_pair(pair: (usize, usize)) -> (usize, usize) {
    let (a, b) = pair;
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// ONLY the pairs themselves are sorted, the whole vector is not sorted.
fn get_sorted_index_pairs(index_pairs: &[(usize, usize)]) -> Vec<(usize, usize)> {
    index_pairs
        .iter()
        .map(|&pair| sort_index_pair(pair))
        .collect()
}

impl PartialEq for Matching {
    // O(n)
    fn eq(&self, other: &Self) -> bool {
        let len = self.index_pairs.len();
        if len != other.index_pairs.len() {
            return false;
        }
        // Only the pairs themselves are sorted, so these are O(n)
        let self_sorted_index_pairs = get_sorted_index_pairs(&self.index_pairs);
        let other_sorted_index_pairs = get_sorted_index_pairs(&other.index_pairs);

        let self_index_hash_map: HashMap<usize, usize> =
            self_sorted_index_pairs.into_iter().collect();

        // self == other iff all pairs in 'other_sorted_index_pairs' match the mapping
        other_sorted_index_pairs.into_iter().all(|(a, b)| {
            let self_b_option = self_index_hash_map.get(&a);
            if let Some(&self_b) = self_b_option {
                self_b == b
            } else {
                false
            }
        })
    }
}

impl Eq for Matching {}

impl Hash for Matching {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        // Only the pairs themselves are sorted, so this is O(n)
        let self_sorted_index_pairs = get_sorted_index_pairs(&self.index_pairs);

        // Sums are order-invariant
        let (sum_a, sum_b) = self_sorted_index_pairs
            .into_iter()
            .fold((0, 0), |(sum_a, sum_b), (a, b)| (sum_a + a, sum_b + b));
        sum_a.hash(state);
        sum_b.hash(state);
    }
}

impl IntoIterator for Matching {
    type Item = (usize, usize);

    type IntoIter = IntoIter<Self::Item>;

    fn into_iter(self) -> Self::IntoIter {
        self.index_pairs.into_iter()
    }
}

impl Matching {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_pairs(pairs: Vec<(usize, usize)>) -> Self {
        Self { index_pairs: pairs }
    }

    pub fn iter(&self) -> Iter<(usize, usize)> {
        self.index_pairs.iter()
    }

    pub fn from_hungarian_result(hungarian_result: Vec<Option<usize>>) -> Self {
        let index_pairs_iter = hungarian_result
            .into_iter()
            .enumerate()
            .map(|(i, j_option)| (i, j_option.unwrap()));
        Self {
            index_pairs: index_pairs_iter.collect(),
        }
    }
}

impl Index<usize> for SquareDistanceMatrix {
    type Output = [f64]; // Output a row for further indexing

    fn index(&self, index: usize) -> &Self::Output {
        &self.distances[(index * self.n)..((index + 1) * self.n)]
    }
}

impl IndexMut<usize> for SquareDistanceMatrix {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        &mut self.distances[(index * self.n)..((index + 1) * self.n)]
    }
}

impl SquareDistanceMatrix {
    /// Create a DistanceMatrix and set the pairwise distances ('set1'-by-'set2')
    /// The behavior is undefined unless 'set1' and 'set2' have the same number of items.
    pub fn from_two_sets(set1: &MatchItemSet, set2: &MatchItemSet) -> Self {
        assert_eq!(set1.len(), set2.len());
        let n = set1.len();

        let mut distances = vec![0.0; n * n];

        for i in 0..n {
            for j in 0..n {
                distances[i * n + j] = set1[i].distance_to(&set2[j]);
            }
        }

        Self { n, distances }
    }

    pub fn into_matching(self) -> Matching {
        let n = self.n;
        let matrix: Vec<u64> = self.distances.into_iter().map(|dist| dist as u64).collect();
        let hungarian_result = hungarian::minimize(&matrix, n, n);
        Matching::from_hungarian_result(hungarian_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f64_approximately(a: f64, b: f64) -> bool {
        (a - b).abs() <= f64::EPSILON
    }

    #[test]
    fn square_distance_matrix_correct_distances() {
        // GIVEN
        let points1 = [
            PointF64::new(0.0, 0.0),
            PointF64::new(0.0, 2.0),
            PointF64::new(3.199, 82.3),
            PointF64::new(9.8, 2.7177),
            PointF64::new(76.2, 35.89),
            PointF64::new(19.84, 85.8),
        ];
        let points2 = [
            PointF64::new(12.68, 29.86),
            PointF64::new(84.6, 20.46),
            PointF64::new(16.2, 214.5),
            PointF64::new(89.64, 23.5),
            PointF64::new(64.7, 29.75),
            PointF64::new(49.72, 83.0),
        ];

        let match_items_iter1 = points1
            .iter()
            .map(|p| MatchItem::new_with_default_id(*p, PointF64::default()));
        let match_items_iter2 = points2
            .iter()
            .map(|p| MatchItem::new_with_default_id(*p, PointF64::default()));

        let match_item_set1 =
            MatchItemSet::from_match_items_and_set_ids(match_items_iter1.collect());
        let match_item_set2 =
            MatchItemSet::from_match_items_and_set_ids(match_items_iter2.collect());

        // WHEN
        let distance_matrix =
            SquareDistanceMatrix::from_two_sets(&match_item_set1, &match_item_set2);

        // THEN
        let n = points1.len();
        for i in 0..n {
            for j in 0..n {
                println!(
                    "{} to {}: {} | {}",
                    i,
                    j,
                    distance_matrix[i][j],
                    points1[i].distance_to(points2[j])
                );
                assert!(f64_approximately(
                    distance_matrix[i][j],
                    points1[i].distance_to(points2[j])
                ));
            }
        }
    }
}
