use anyhow::Result;
use petgraph::graph::DiGraph;
use petgraph::algo::toposort;
use std::collections::{HashMap, HashSet};
use crate::model::*;

/// Dependency entry: a resource type and the resource types it references.
pub type DependencyMap = Vec<(String, Vec<String>)>;

/// Extract dependencies from StructureDefinitions.
///
/// Scans each profile's snapshot for Reference types with targetProfile,
/// building a map of resource_type → [referenced_resource_types].
pub fn extract_dependencies(profiles: &[StructureDefinition]) -> DependencyMap {
    let mut deps: DependencyMap = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for profile in profiles {
        let resource_type = &profile.base_type;
        if seen.contains(resource_type) {
            continue;
        }
        seen.insert(resource_type.clone());

        let mut references: HashSet<String> = HashSet::new();

        if let Some(snapshot) = &profile.snapshot {
            for element in &snapshot.element {
                for type_def in &element.type_ {
                    if type_def.code == "Reference" {
                        for target in &type_def.target_profile {
                            // Extract resource type from profile URL
                            // e.g., "http://hl7.org/fhir/StructureDefinition/Patient" → "Patient"
                            let ref_type = target.rsplit('/').next().unwrap_or("Resource");
                            if ref_type != "Resource" && ref_type != resource_type {
                                references.insert(ref_type.to_string());
                            }
                        }
                        // If no target profiles, it's a generic reference — skip
                        if type_def.target_profile.is_empty() {
                            // Could reference anything, not helpful for ordering
                        }
                    }
                }
            }
        }

        deps.push((resource_type.clone(), references.into_iter().collect()));
    }

    deps
}

/// Resolve creation order from a dependency map using topological sort.
///
/// Returns an error if circular dependencies are detected.
pub fn resolve_creation_order(deps: &DependencyMap) -> Result<Vec<String>> {
    let mut graph = DiGraph::<String, ()>::new();
    let mut node_indices: HashMap<String, petgraph::graph::NodeIndex> = HashMap::new();

    // Add nodes for all resource types
    for (resource_type, _) in deps {
        let idx = graph.add_node(resource_type.clone());
        node_indices.insert(resource_type.clone(), idx);
    }

    // Add edges: if A depends on B, then B must be created before A
    // Edge direction: A → B means "A depends on B"
    for (resource_type, references) in deps {
        let from_idx = node_indices.get(resource_type).unwrap();
        for dep in references {
            if let Some(to_idx) = node_indices.get(dep) {
                graph.add_edge(*from_idx, *to_idx, ());
            }
        }
    }

    // Topological sort gives us an order where dependencies come first
    let sorted = toposort(&graph, None)
        .map_err(|cycle| {
            let node = cycle.node_id();
            anyhow::anyhow!(
                "Circular dependency detected involving: {}",
                graph[node]
            )
        })?;

    // Reverse because toposort puts dependencies last (dependents first)
    // We want dependencies first (create Patient before Observation)
    let mut order: Vec<String> = sorted
        .into_iter()
        .rev()
        .map(|idx| graph[idx].clone())
        .collect();

    // Also add resource types that weren't in the dependency map but were referenced
    // (they might not have their own profiles but still need to be created)
    let all_referenced: HashSet<String> = deps
        .iter()
        .flat_map(|(_, refs)| refs.iter())
        .cloned()
        .collect();
    let existing: HashSet<String> = order.iter().cloned().collect();

    for resource_type in all_referenced {
        if !existing.contains(&resource_type) {
            order.push(resource_type);
        }
    }

    Ok(order)
}

/// Merge auto-resolved creation order with user-specified overrides.
///
/// User overrides take precedence: if the user specifies an order, resources
/// appear in that order. Resources not in the user list are appended in
/// auto-resolved order.
pub fn merge_creation_order(
    auto_order: &[String],
    user_order: &[String],
) -> Vec<String> {
    if user_order.is_empty() {
        return auto_order.to_vec();
    }

    let mut result: Vec<String> = user_order.to_vec();
    let user_set: HashSet<String> = user_order.iter().cloned().collect();

    for resource_type in auto_order {
        if !user_set.contains(resource_type) && !result.contains(resource_type) {
            result.push(resource_type.clone());
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_simple_dependency() {
        let deps = vec![
            ("Observation".to_string(), vec!["Patient".to_string(), "Encounter".to_string()]),
            ("Encounter".to_string(), vec!["Patient".to_string()]),
            ("Patient".to_string(), vec![]),
        ];
        let order = resolve_creation_order(&deps).unwrap();

        let patient_idx = order.iter().position(|r| r == "Patient").unwrap();
        let encounter_idx = order.iter().position(|r| r == "Encounter").unwrap();
        let observation_idx = order.iter().position(|r| r == "Observation").unwrap();

        assert!(patient_idx < encounter_idx, "Patient should come before Encounter");
        assert!(patient_idx < observation_idx, "Patient should come before Observation");
        assert!(encounter_idx < observation_idx, "Encounter should come before Observation");
    }

    #[test]
    fn detect_circular_dependency() {
        let deps = vec![
            ("A".to_string(), vec!["B".to_string()]),
            ("B".to_string(), vec!["C".to_string()]),
            ("C".to_string(), vec!["A".to_string()]),
        ];
        let result = resolve_creation_order(&deps);
        assert!(result.is_err(), "Should detect circular dependency");
    }

    #[test]
    fn resolve_no_dependencies() {
        let deps = vec![
            ("Patient".to_string(), vec![]),
            ("Observation".to_string(), vec![]),
        ];
        let order = resolve_creation_order(&deps).unwrap();
        assert_eq!(order.len(), 2);
        assert!(order.contains(&"Patient".to_string()));
        assert!(order.contains(&"Observation".to_string()));
    }

    #[test]
    fn merge_creation_order_with_override() {
        let auto = vec!["Patient".to_string(), "Encounter".to_string(), "Observation".to_string()];
        let user = vec!["Encounter".to_string(), "Patient".to_string()];
        let merged = merge_creation_order(&auto, &user);

        assert_eq!(merged[0], "Encounter");
        assert_eq!(merged[1], "Patient");
        assert_eq!(merged[2], "Observation");
    }

    #[test]
    fn merge_creation_order_empty_override() {
        let auto = vec!["Patient".to_string(), "Encounter".to_string()];
        let user: Vec<String> = vec![];
        let merged = merge_creation_order(&auto, &user);
        assert_eq!(merged, auto);
    }

    #[test]
    fn extract_dependencies_from_profiles() {
        let profile = StructureDefinition {
            resource_type: "StructureDefinition".to_string(),
            url: "http://example.org/TestObservation".to_string(),
            base_type: "Observation".to_string(),
            name: "TestObservation".to_string(),
            kind: "resource".to_string(),
            derivation: Some("constraint".to_string()),
            base_definition: None,
            snapshot: Some(Snapshot {
                element: vec![
                    ElementDefinition {
                        id: "Observation".to_string(),
                        path: "Observation".to_string(),
                        min: Some(0),
                        max: Some("*".to_string()),
                        type_: vec![],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: false,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                    },
                    ElementDefinition {
                        id: "Observation.subject".to_string(),
                        path: "Observation.subject".to_string(),
                        min: Some(1),
                        max: Some("1".to_string()),
                        type_: vec![ElementDefinitionType {
                            code: "Reference".to_string(),
                            target_profile: vec![
                                "http://hl7.org/fhir/StructureDefinition/Patient".to_string(),
                            ],
                            versioning: None,
                        }],
                        fixed_string: None,
                        fixed_uri: None,
                        fixed_code: None,
                        fixed_boolean: None,
                        fixed_integer: None,
                        fixed_decimal: None,
                        pattern_string: None,
                        pattern_uri: None,
                        pattern_code: None,
                        pattern_boolean: None,
                        must_support: true,
                        short: None,
                        definition: None,
                        binding: None,
                        content_reference: None,
                        fixed_quantity: None,
                        pattern_quantity: None,
                        fixed_coding: None,
                        pattern_coding: None,
                        fixed_codeable_concept: None,
                        pattern_codeable_concept: None,
                        constraint: vec![],
                        is_modifier: false,
                        is_summary: false,
                    },
                ],
            }),
            differential: None,
        };

        let deps = extract_dependencies(&[profile]);
        assert_eq!(deps.len(), 1);
        assert_eq!(deps[0].0, "Observation");
        assert!(deps[0].1.contains(&"Patient".to_string()));
    }
}