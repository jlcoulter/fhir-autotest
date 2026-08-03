pub mod boundary;
pub mod cardinality;
pub mod encoding;
pub mod search_param;
pub mod type_mismatch;

pub use boundary::BoundaryMutator;
pub use cardinality::CardinalityMutator;
pub use encoding::EncodingMutator;
pub use search_param::SearchParamMutator;
pub use type_mismatch::TypeMismatchMutator;

use fhir_autotest::model::profile::StructureDefinition;

/// A mutation strategy that transforms a valid FHIR resource into a fuzzed variant.
pub trait Mutator: Send + Sync {
    /// Human-readable name for this mutator category.
    fn name(&self) -> &'static str;

    /// Produce a fuzzed variant of `base_resource`.
    ///
    /// `profile` provides the StructureDefinition context so mutations can be
    /// type-aware (e.g. know which fields are strings vs integers).
    /// `seed` enables deterministic mutation sequences.
    fn mutate(
        &self,
        base_resource: &serde_json::Value,
        profile: &StructureDefinition,
        seed: u64,
    ) -> serde_json::Value;
}
