//! An in-memory vector store with exact cosine search.
//!
//! Exact search over a few thousand chunks needs no index; past that, this is the part to replace (pgvector, an HNSW
//! library). The interface is the same: add vectors, ask for the nearest ones above a floor.

use serde::Serialize;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("vector has {got} dimensions, the store holds {expected}")]
    Dimensions { expected: usize, got: usize },
    #[error("vector contains NaN or infinity")]
    NotFinite,
}

#[derive(Debug, Clone, Serialize)]
pub struct Stored {
    pub doc: String,
    pub heading: String,
    pub text: String,
    #[serde(skip)]
    vector: Vec<f32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub score: f32,
    pub doc: String,
    pub heading: String,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct VectorStore {
    entries: Vec<Stored>,
    dimensions: Option<usize>,
}

impl VectorStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.dimensions = None;
    }

    /// # Errors
    /// The vector has the wrong length or a non-finite component.
    pub fn add(
        &mut self,
        doc: &str,
        heading: &str,
        text: &str,
        vector: Vec<f32>,
    ) -> Result<(), StoreError> {
        if vector.iter().any(|x| !x.is_finite()) {
            return Err(StoreError::NotFinite);
        }
        let expected = *self.dimensions.get_or_insert(vector.len());
        if vector.len() != expected {
            return Err(StoreError::Dimensions {
                expected,
                got: vector.len(),
            });
        }
        self.entries.push(Stored {
            doc: doc.to_owned(),
            heading: heading.to_owned(),
            text: text.to_owned(),
            vector,
        });
        Ok(())
    }

    /// The `k` nearest entries whose cosine similarity is at least `floor`, best first. Ties are broken by document and
    /// heading, so the order never depends on insertion order.
    ///
    /// # Errors
    /// The query has the wrong length.
    pub fn search(&self, query: &[f32], k: usize, floor: f32) -> Result<Vec<Hit>, StoreError> {
        if let Some(expected) = self.dimensions
            && query.len() != expected
        {
            return Err(StoreError::Dimensions {
                expected,
                got: query.len(),
            });
        }
        let mut hits: Vec<Hit> = self
            .entries
            .iter()
            .map(|e| {
                (
                    e,
                    e.vector.iter().zip(query).map(|(a, b)| a * b).sum::<f32>(),
                )
            })
            .filter(|(_, score)| score.is_finite() && *score >= floor)
            .map(|(e, score)| Hit {
                score,
                doc: e.doc.clone(),
                heading: e.heading.clone(),
                text: e.text.clone(),
            })
            .collect();
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then_with(|| a.doc.cmp(&b.doc))
                .then_with(|| a.heading.cmp(&b.heading))
        });
        hits.truncate(k);
        Ok(hits)
    }

    /// Best score regardless of the floor, so an "I do not know" can say how close the nearest thing was.
    #[must_use]
    pub fn best_score(&self, query: &[f32]) -> Option<f32> {
        self.entries
            .iter()
            .filter(|e| e.vector.len() == query.len())
            .map(|e| e.vector.iter().zip(query).map(|(a, b)| a * b).sum::<f32>())
            .filter(|s| s.is_finite())
            .max_by(f32::total_cmp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_nearest_first_and_respects_k_and_the_floor() {
        let mut s = VectorStore::new();
        s.add("a", "", "x", vec![1.0, 0.0]).expect("add");
        s.add("b", "", "y", vec![0.6, 0.8]).expect("add");
        s.add("c", "", "z", vec![0.0, 1.0]).expect("add");
        let hits = s.search(&[1.0, 0.0], 2, 0.5).expect("search");
        assert_eq!(
            hits.iter().map(|h| h.doc.as_str()).collect::<Vec<_>>(),
            ["a", "b"]
        );
        assert!(s.search(&[1.0, 0.0], 5, 0.99).expect("search").len() == 1);
    }

    #[test]
    fn equal_scores_are_ordered_by_name_not_by_insertion() {
        let mut s = VectorStore::new();
        s.add("zeta", "", "1", vec![1.0, 0.0]).expect("add");
        s.add("alpha", "", "2", vec![1.0, 0.0]).expect("add");
        let hits = s.search(&[1.0, 0.0], 5, 0.0).expect("search");
        assert_eq!(hits[0].doc, "alpha");
    }

    #[test]
    fn wrong_sizes_and_non_finite_numbers_are_refused() {
        let mut s = VectorStore::new();
        s.add("a", "", "x", vec![1.0, 0.0]).expect("add");
        assert_eq!(
            s.add("b", "", "y", vec![1.0]),
            Err(StoreError::Dimensions {
                expected: 2,
                got: 1
            })
        );
        assert_eq!(
            s.add("c", "", "z", vec![f32::NAN, 0.0]),
            Err(StoreError::NotFinite)
        );
        assert!(s.search(&[1.0], 1, 0.0).is_err());
    }

    #[test]
    fn an_empty_store_finds_nothing() {
        let s = VectorStore::new();
        assert!(s.search(&[1.0], 3, 0.0).expect("search").is_empty());
        assert_eq!(s.best_score(&[1.0]), None);
    }
}
