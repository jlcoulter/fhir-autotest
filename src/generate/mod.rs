pub mod bulk_data;
pub mod conformance;
pub mod dependency_resolver;
pub mod hcpd;
pub mod locality;
pub mod model;
pub mod planner;
pub mod resource_generator;
pub mod value_resolver;

pub use bulk_data::*;
pub use conformance::*;
pub use dependency_resolver::*;
pub use hcpd::*;
pub use locality::*;
pub use model::*;
pub use planner::*;
pub use resource_generator::*;

// ---------------------------------------------------------------------------
// FHIR data types that are not independently creatable resources.
// Some CapabilityStatements list types like Extension or Identifier which
// are structural types, not top-level FHIR resources.
// ---------------------------------------------------------------------------
pub const NON_RESOURCE_TYPES: &[&str] = &[
    "Extension",
    "Identifier",
    "Coding",
    "CodeableConcept",
    "Address",
    "HumanName",
    "ContactPoint",
    "Period",
    "Quantity",
    "Range",
    "Ratio",
    "Attachment",
    "Annotation",
    "Signature",
    "Timing",
    // Parameters is a special FHIR resource type used only as an
    // operation request/response container — it is not a persistable
    // resource and servers correctly return 404 for CRUD/search.
    "Parameters",
];
