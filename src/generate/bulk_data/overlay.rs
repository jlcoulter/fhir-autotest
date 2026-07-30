use rand::Rng;
use serde_json;

use super::random_ref;

/// Overlay cross-references onto a profile-generated resource.
///
/// When `generate_resource` produces a resource from a StructureDefinition,
/// it creates required fields but cannot know about the IDs of other
/// resources in the bulk data set. This function fills in cross-references
/// (practitioner, organization, location, healthcareService) that the
/// profile may require but which depend on other generated resources.
#[allow(clippy::too_many_arguments)]
pub fn overlay_cross_references(
    resource: &mut serde_json::Value,
    resource_type: &str,
    _id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    hs_ids: &[String],
    practitioner_role_ids: &[String],
    endpoint_ids: &[String],
    rng: &mut impl Rng,
) {
    let obj = match resource.as_object_mut() {
        Some(o) => o,
        None => return,
    };

    match resource_type {
        "PractitionerRole" => {
            // HCPD requires practitioner, organization, and healthcareService references.
            // practitioner and organization are always required; healthcareService
            // references depend on HealthcareService IDs which may not exist yet.
            // Ensure deterministic coverage for _revinclude=PractitionerRole:practitioner
            // on practitioner-1.
            if !prac_ids.is_empty() {
                let ref_str = if _id == "practitionerrole-1"
                    && prac_ids.iter().any(|id| id == "practitioner-1")
                {
                    "Practitioner/practitioner-1".to_string()
                } else {
                    random_ref("Practitioner", prac_ids, rng)
                };
                obj.insert(
                    "practitioner".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "organization".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !hs_ids.is_empty() {
                // Always reference the first HealthcareService ID to avoid
                // HAPI-1096 ("Resource is deleted") — random HealthcareService
                // references may point to resources deleted by a previous test
                // run's cleanup, which HAPI permanently marks as deleted and
                // refuses new references to.
                let ref_str = format!("HealthcareService/{}", hs_ids[0]);
                obj.insert(
                    "healthcareService".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !endpoint_ids.is_empty() {
                let ref_str = random_ref("Endpoint", endpoint_ids, rng);
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
        }
        "Location" if !org_ids.is_empty() => {
            // Ensure at least one deterministic reverse include path for
            // Organization/_revinclude=Location:organization on organization-1.
            let ref_str = if _id == "location-1" && org_ids.iter().any(|id| id == "organization-1")
            {
                "Organization/organization-1".to_string()
            } else {
                random_ref("Organization", org_ids, rng)
            };
            obj.insert(
                "managingOrganization".to_string(),
                serde_json::json!({ "reference": ref_str }),
            );
            // Ensure deterministic coverage for _revinclude=Location:endpoint
            // on endpoint-1.
            if !endpoint_ids.is_empty() {
                let endpoint_ref =
                    if _id == "location-1" && endpoint_ids.iter().any(|id| id == "endpoint-1") {
                        "Endpoint/endpoint-1".to_string()
                    } else {
                        random_ref("Endpoint", endpoint_ids, rng)
                    };
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": endpoint_ref }]),
                );
            }
        }
        "HealthcareService" => {
            if !org_ids.is_empty() {
                let ref_str = random_ref("Organization", org_ids, rng);
                obj.insert(
                    "providedBy".to_string(),
                    serde_json::json!({ "reference": ref_str }),
                );
            }
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "location".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            if !endpoint_ids.is_empty() {
                let ref_str = random_ref("Endpoint", endpoint_ids, rng);
                obj.insert(
                    "endpoint".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            }
            // Replace coverageArea references — the profile-aware generator
            // extracts the resource type from the target profile URL, but
            // profile names like "hcpd-service-coverage-area" are not valid
            // FHIR resource types. coverageArea should reference Location.
            if !loc_ids.is_empty() {
                let ref_str = random_ref("Location", loc_ids, rng);
                obj.insert(
                    "coverageArea".to_string(),
                    serde_json::json!([{ "reference": ref_str }]),
                );
            } else {
                obj.remove("coverageArea");
            }

            // Populate the mustSupport `telecom.extension` (contact-purpose),
            // which the profile-driven generator skips because it is an
            // Extension-typed field.
            ensure_telecom_contact_purpose(obj);
        }
        "Endpoint" if !org_ids.is_empty() => {
            let ref_str = random_ref("Organization", org_ids, rng);
            obj.insert(
                "managingOrganization".to_string(),
                serde_json::json!({ "reference": ref_str }),
            );
        }
        "Organization" if !org_ids.is_empty() => {
            // Give only a small fraction of Organizations a partOf, and always
            // point at an *earlier*-indexed Organization. This keeps the
            // hierarchy sparse and, crucially, acyclic: references only ever go
            // backward, so there is a valid upload order (guaranteed by the
            // wave ordering in upload_ndjson_files) and no reference cycles can
            // form. Most Organizations are therefore roots with no parent.
            //
            // Exception: the conformance must_support test always queries
            // `organization-1`, so that resource must exhibit every mustSupport
            // field — including partOf. Point it deterministically at
            // `organization-2` (which exists whenever there is more than one
            // Organization). This is the only forward reference; the upload
            // wave ordering commits organization-2 before organization-1, so it
            // resolves at upload time. `organization-2` is forced to be a root
            // (no partOf) so organization-1 → organization-2 cannot form a cycle.
            const PART_OF_PROBABILITY: f64 = 0.01; // ~1 in 100
            const MUST_SUPPORT_ANCHOR: &str = "organization-1";
            const ANCHOR_PARENT: &str = "organization-2";
            if _id == MUST_SUPPORT_ANCHOR {
                if org_ids.iter().any(|id| id.as_str() == ANCHOR_PARENT) {
                    obj.insert(
                        "partOf".to_string(),
                        serde_json::json!({ "reference": format!("Organization/{ANCHOR_PARENT}") }),
                    );
                } else {
                    obj.remove("partOf");
                }
            } else if _id == ANCHOR_PARENT {
                // Keep the anchor's parent a root to avoid a reference cycle.
                obj.remove("partOf");
            } else {
                let self_index = org_ids.iter().position(|id| id.as_str() == _id);
                match self_index {
                    Some(idx) if idx > 0 && rng.random_bool(PART_OF_PROBABILITY) => {
                        // Pick a random Organization that appears before this one.
                        let parent = &org_ids[rng.random_range(0..idx)];
                        obj.insert(
                            "partOf".to_string(),
                            serde_json::json!({ "reference": format!("Organization/{parent}") }),
                        );
                    }
                    _ => {
                        obj.remove("partOf");
                    }
                }
            }
        }
        "Provenance" => {
            let target_ref = provenance_target_for_id(
                _id,
                org_ids,
                prac_ids,
                loc_ids,
                hs_ids,
                practitioner_role_ids,
                endpoint_ids,
                rng,
            );

            if let Some(ref_str) = target_ref.as_deref() {
                // Preserve mustSupport sub-fields (e.g. target.extension)
                // that were populated during resource generation.
                // Merge the reference into the first existing target
                // object rather than replacing the entire array.
                if let Some(targets) = obj.get_mut("target").and_then(|t| t.as_array_mut()) {
                    if let Some(first) = targets.first_mut()
                        && let Some(target_obj) = first.as_object_mut()
                    {
                        target_obj.insert("reference".to_string(), serde_json::json!(ref_str));
                        ensure_target_path_extension(target_obj);
                    }
                } else {
                    obj.insert(
                        "target".to_string(),
                        serde_json::json!([{
                            "reference": ref_str,
                            "extension": [{
                                "url": "http://hl7.org/fhir/StructureDefinition/targetPath",
                                "valueString": "id"
                            }]
                        }]),
                    );
                }
            }

            // Override activity with a valid code from the Provenance activity type
            // value set to avoid Terminology_TX_NoValid_2_CC warnings.
            obj.insert(
                "activity".to_string(),
                serde_json::json!({
                    "coding": [{
                        "system": "http://terminology.hl7.org/CodeSystem/provenance-activity-type",
                        "code": "CREATE"
                    }]
                }),
            );

            if !org_ids.is_empty() {
                obj.insert(
                    "agent".to_string(),
                    serde_json::json!([
                        {
                            "who": {
                                "reference": random_ref("Organization", org_ids, rng)
                            }
                        }
                    ]),
                );
            }

            // entity.what must reference a stable resource that won't be
            // deleted during the test run. Using the same reference as
            // target risks HAPI-1096 ("Resource is deleted") when a
            // DELETE test removes that resource. Use Organization
            // (a root-level resource) instead.
            if !org_ids.is_empty() {
                obj.insert(
                    "entity".to_string(),
                    serde_json::json!([
                        {
                            "role": "source",
                            "what": {
                                "reference": random_ref("Organization", org_ids, rng)
                            }
                        }
                    ]),
                );
            }
        }
        _ => {}
    }
}

/// Ensure a Provenance `target` object carries the mustSupport `target.extension`.
///
/// The HCPD Provenance profile marks `Provenance.target.extension` as
/// mustSupport, but the profile-driven generator skips Extension-typed fields
/// because it cannot invent a valid extension URL. The standard `targetPath`
/// extension (which records the element a change applied to) satisfies the
/// mustSupport presence check; `id` is a FHIRPath that is valid on every target
/// resource type. Only added when no extension is already present.
fn ensure_target_path_extension(target_obj: &mut serde_json::Map<String, serde_json::Value>) {
    if target_obj.contains_key("extension") {
        return;
    }
    target_obj.insert(
        "extension".to_string(),
        serde_json::json!([{
            "url": "http://hl7.org/fhir/StructureDefinition/targetPath",
            "valueString": "id"
        }]),
    );
}

/// Ensure a HealthcareService carries the mustSupport `telecom.extension`.
///
/// The HCPD HealthcareService profile slices `telecom.extension` on the AU Base
/// `contact-purpose` extension (mustSupport). The profile-driven generator
/// skips Extension-typed fields, so the generated `telecom` has no extension
/// and the conformance presence check fails. Add the `contact-purpose`
/// extension to the first telecom entry (creating a default entry if none
/// exists). The value binding is extensible, so the `UNK` NullFlavor coding —
/// the fallback used elsewhere in this generator — is accepted.
fn ensure_telecom_contact_purpose(obj: &mut serde_json::Map<String, serde_json::Value>) {
    let contact_purpose = serde_json::json!({
        "url": "http://hl7.org.au/fhir/StructureDefinition/contact-purpose",
        "valueCodeableConcept": {
            "coding": [{
                "system": "http://terminology.hl7.org/CodeSystem/v3-NullFlavor",
                "code": "UNK"
            }],
            "text": "unknown"
        }
    });

    let telecoms = obj
        .entry("telecom")
        .or_insert_with(|| serde_json::json!([{ "system": "phone", "value": "555-0000" }]));

    let Some(telecoms) = telecoms.as_array_mut() else {
        return;
    };
    if telecoms.is_empty() {
        telecoms.push(serde_json::json!({ "system": "phone", "value": "555-0000" }));
    }
    if let Some(first) = telecoms.first_mut()
        && let Some(first_obj) = first.as_object_mut()
        && !first_obj.contains_key("extension")
    {
        first_obj.insert(
            "extension".to_string(),
            serde_json::json!([contact_purpose]),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn provenance_target_for_id(
    _provenance_id: &str,
    org_ids: &[String],
    prac_ids: &[String],
    loc_ids: &[String],
    hs_ids: &[String],
    practitioner_role_ids: &[String],
    endpoint_ids: &[String],
    rng: &mut impl Rng,
) -> Option<String> {
    // Distribute Provenance targets across all resource types so that
    // _revinclude=Provenance:target queries on Location, HealthcareService,
    // PractitionerRole, etc. return matching Provenance resources.
    // Use a round-robin approach based on the provenance id to get
    // deterministic coverage for the first few resources.
    let pools: Vec<(&[String], &str)> = vec![
        (org_ids, "Organization"),
        (prac_ids, "Practitioner"),
        (loc_ids, "Location"),
        (hs_ids, "HealthcareService"),
        (practitioner_role_ids, "PractitionerRole"),
        (endpoint_ids, "Endpoint"),
    ];
    let non_empty: Vec<(&[String], &str)> = pools
        .into_iter()
        .filter(|(ids, _)| !ids.is_empty())
        .collect();
    if non_empty.is_empty() {
        return None;
    }
    // Use the provenance id suffix to pick a pool deterministically
    let idx = _provenance_id
        .rsplit('-')
        .next()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        % non_empty.len();
    let (pool, rtype) = non_empty[idx];
    Some(random_ref(rtype, pool, rng))
}
