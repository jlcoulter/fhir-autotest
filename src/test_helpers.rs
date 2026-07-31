use std::io::Write;

/// Create a minimal FHIR IG package (.tgz) for testing.
/// Contains a CapabilityStatement with Patient and Observation resources,
/// plus their StructureDefinitions and a SearchParameter.
pub fn create_test_ig_package() -> Vec<u8> {
    let cs_json = r#"{
        "resourceType": "CapabilityStatement",
        "url": "http://example.org/CapabilityStatement/TestIG",
        "name": "TestIG",
        "status": "active",
        "rest": [{
            "mode": "server",
            "resource": [{
                "type": "Patient",
                "profile": "http://hl7.org/fhir/StructureDefinition/Patient",
                "supportedProfile": ["http://example.org/StructureDefinition/TestPatient"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"},
                    {"code": "update"},
                    {"code": "delete"}
                ],
                "searchParam": [
                    {"name": "name", "type": "string"},
                    {"name": "birthdate", "type": "date"}
                ]
            }, {
                "type": "Observation",
                "profile": "http://hl7.org/fhir/StructureDefinition/Observation",
                "supportedProfile": ["http://example.org/StructureDefinition/TestObservation"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"}
                ],
                "searchParam": [
                    {"name": "category", "type": "token"},
                    {"name": "code", "type": "token"}
                ]
            }],
            "interaction": []
        }]
    }"#;

    let patient_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/TestPatient",
        "name": "TestPatient",
        "type": "Patient",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Patient",
                "path": "Patient",
                "min": 0,
                "max": "*"
            }, {
                "id": "Patient.id",
                "path": "Patient.id",
                "min": 0,
                "max": "1",
                "type": [{"code": "id"}]
            }, {
                "id": "Patient.identifier",
                "path": "Patient.identifier",
                "min": 1,
                "max": "*",
                "type": [{"code": "Identifier"}],
                "mustSupport": true
            }, {
                "id": "Patient.name",
                "path": "Patient.name",
                "min": 1,
                "max": "*",
                "type": [{"code": "HumanName"}],
                "mustSupport": true
            }, {
                "id": "Patient.gender",
                "path": "Patient.gender",
                "min": 0,
                "max": "1",
                "type": [{"code": "code"}]
            }, {
                "id": "Patient.birthDate",
                "path": "Patient.birthDate",
                "min": 0,
                "max": "1",
                "type": [{"code": "date"}]
            }]
        }
    }"#;

    let observation_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/TestObservation",
        "name": "TestObservation",
        "type": "Observation",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Observation",
                "path": "Observation",
                "min": 0,
                "max": "*"
            }, {
                "id": "Observation.id",
                "path": "Observation.id",
                "min": 0,
                "max": "1",
                "type": [{"code": "id"}]
            }, {
                "id": "Observation.status",
                "path": "Observation.status",
                "min": 1,
                "max": "1",
                "type": [{"code": "code"}],
                "fixedCode": "final"
            }, {
                "id": "Observation.subject",
                "path": "Observation.subject",
                "min": 1,
                "max": "1",
                "type": [{
                    "code": "Reference",
                    "targetProfile": ["http://hl7.org/fhir/StructureDefinition/Patient"]
                }],
                "mustSupport": true
            }, {
                "id": "Observation.code",
                "path": "Observation.code",
                "min": 1,
                "max": "1",
                "type": [{"code": "CodeableConcept"}]
            }, {
                "id": "Observation.valueString",
                "path": "Observation.valueString",
                "min": 0,
                "max": "1",
                "type": [{"code": "string"}]
            }]
        }
    }"#;

    let sp_json = r#"{
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-name",
        "name": "name",
        "code": "name",
        "base": ["Patient"],
        "type": "string",
        "expression": "Patient.name"
    }"#;

    let mut tar_data = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_data);

        let files = [
            ("package/CapabilityStatement-test.json", cs_json),
            (
                "package/StructureDefinition-TestPatient.json",
                patient_sd_json,
            ),
            (
                "package/StructureDefinition-TestObservation.json",
                observation_sd_json,
            ),
            ("package/SearchParameter-patient-name.json", sp_json),
        ];

        for (path, content) in &files {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, *path, content.as_bytes())
                .unwrap();
        }

        tar.finish().unwrap();
    }

    let mut gz_data = Vec::new();
    {
        let mut gz = flate2::write::GzEncoder::new(&mut gz_data, flate2::Compression::default());
        gz.write_all(&tar_data).unwrap();
        gz.finish().unwrap();
    }

    gz_data
}

/// Create an extended FHIR IG package (.tgz) that exercises more complex
/// profile features: sliced identifiers, extensions, value set bindings,
/// profiled type chains, and composite search parameters.
///
/// This package includes:
/// - A CapabilityStatement with Patient and Observation
/// - A Patient profile with a sliced identifier (IHPII-style) and an extension
/// - An Observation profile with a value set bound field
/// - A ValueSet and CodeSystem for the binding
/// - An extension definition for the Patient extension
/// - A composite SearchParameter
pub fn create_extended_test_ig_package() -> Vec<u8> {
    let cs_json = r#"{
        "resourceType": "CapabilityStatement",
        "url": "http://example.org/CapabilityStatement/ExtendedIG",
        "name": "ExtendedIG",
        "status": "active",
        "rest": [{
            "mode": "server",
            "resource": [{
                "type": "Patient",
                "profile": "http://hl7.org/fhir/StructureDefinition/Patient",
                "supportedProfile": ["http://example.org/StructureDefinition/ExtendedPatient"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"},
                    {"code": "update"},
                    {"code": "delete"}
                ],
                "searchParam": [
                    {"name": "name", "type": "string"},
                    {"name": "birthdate", "type": "date"},
                    {"name": "identifier", "type": "token"}
                ]
            }, {
                "type": "Observation",
                "profile": "http://hl7.org/fhir/StructureDefinition/Observation",
                "supportedProfile": ["http://example.org/StructureDefinition/ExtendedObservation"],
                "interaction": [
                    {"code": "read"},
                    {"code": "search-type"},
                    {"code": "create"}
                ],
                "searchParam": [
                    {"name": "category", "type": "token"},
                    {"name": "code", "type": "token"},
                    {"name": "status", "type": "token"}
                ]
            }],
            "interaction": []
        }]
    }"#;

    // ExtendedPatient profile with:
    // 1. A sliced identifier (IHPII-style, discriminator on system)
    // 2. An extension (birthSex) with a value set binding
    // 3. A profiled type reference to BasePatient
    let extended_patient_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/ExtendedPatient",
        "name": "ExtendedPatient",
        "type": "Patient",
        "kind": "resource",
        "derivation": "constraint",
        "baseDefinition": "http://example.org/StructureDefinition/BasePatient",
        "snapshot": {
            "element": [{
                "id": "Patient",
                "path": "Patient",
                "min": 0,
                "max": "*"
            }, {
                "id": "Patient.id",
                "path": "Patient.id",
                "min": 0,
                "max": "1",
                "type": [{"code": "id"}]
            }, {
                "id": "Patient.identifier",
                "path": "Patient.identifier",
                "min": 1,
                "max": "*",
                "type": [{"code": "Identifier"}],
                "mustSupport": true,
                "slicing": {
                    "discriminator": [{"type": "pattern", "path": "system"}],
                    "rules": "open",
                    "description": "Slice on system"
                }
            }, {
                "id": "Patient.identifier:ihpii",
                "path": "Patient.identifier",
                "sliceName": "ihpii",
                "min": 1,
                "max": "1",
                "type": [{
                    "code": "Identifier",
                    "profile": ["http://example.org/StructureDefinition/IHPII-Identifier"]
                }],
                "mustSupport": true
            }, {
                "id": "Patient.identifier:ihpii.system",
                "path": "Patient.identifier.system",
                "min": 1,
                "max": "1",
                "type": [{"code": "uri"}],
                "patternUri": "http://ns.electronichealth.net.au/id/hpii/1.0"
            }, {
                "id": "Patient.identifier:ihpii.value",
                "path": "Patient.identifier.value",
                "min": 1,
                "max": "1",
                "type": [{"code": "string"}]
            }, {
                "id": "Patient.name",
                "path": "Patient.name",
                "min": 1,
                "max": "*",
                "type": [{"code": "HumanName"}],
                "mustSupport": true
            }, {
                "id": "Patient.gender",
                "path": "Patient.gender",
                "min": 0,
                "max": "1",
                "type": [{"code": "code"}]
            }, {
                "id": "Patient.birthDate",
                "path": "Patient.birthDate",
                "min": 0,
                "max": "1",
                "type": [{"code": "date"}]
            }, {
                "id": "Patient.extension",
                "path": "Patient.extension",
                "slicing": {
                    "discriminator": [{"type": "value", "path": "url"}],
                    "rules": "open"
                }
            }, {
                "id": "Patient.extension:birthSex",
                "path": "Patient.extension",
                "sliceName": "birthSex",
                "min": 0,
                "max": "1",
                "type": [{
                    "code": "Extension",
                    "profile": ["http://example.org/StructureDefinition/BirthSexExtension"]
                }]
            }]
        }
    }"#;

    // BasePatient profile that ExtendedPatient derives from
    let base_patient_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/BasePatient",
        "name": "BasePatient",
        "type": "Patient",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Patient",
                "path": "Patient",
                "min": 0,
                "max": "*"
            }, {
                "id": "Patient.id",
                "path": "Patient.id",
                "min": 0,
                "max": "1",
                "type": [{"code": "id"}]
            }, {
                "id": "Patient.identifier",
                "path": "Patient.identifier",
                "min": 1,
                "max": "*",
                "type": [{"code": "Identifier"}],
                "mustSupport": true
            }, {
                "id": "Patient.name",
                "path": "Patient.name",
                "min": 1,
                "max": "*",
                "type": [{"code": "HumanName"}],
                "mustSupport": true
            }, {
                "id": "Patient.birthDate",
                "path": "Patient.birthDate",
                "min": 0,
                "max": "1",
                "type": [{"code": "date"}]
            }]
        }
    }"#;

    // ExtendedObservation profile with a value set bound field
    let extended_observation_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/ExtendedObservation",
        "name": "ExtendedObservation",
        "type": "Observation",
        "kind": "resource",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Observation",
                "path": "Observation",
                "min": 0,
                "max": "*"
            }, {
                "id": "Observation.id",
                "path": "Observation.id",
                "min": 0,
                "max": "1",
                "type": [{"code": "id"}]
            }, {
                "id": "Observation.status",
                "path": "Observation.status",
                "min": 1,
                "max": "1",
                "type": [{"code": "code"}],
                "binding": {
                    "strength": "required",
                    "valueSet": "http://example.org/ValueSet/ObservationStatus"
                }
            }, {
                "id": "Observation.subject",
                "path": "Observation.subject",
                "min": 1,
                "max": "1",
                "type": [{
                    "code": "Reference",
                    "targetProfile": ["http://hl7.org/fhir/StructureDefinition/Patient"]
                }],
                "mustSupport": true
            }, {
                "id": "Observation.code",
                "path": "Observation.code",
                "min": 1,
                "max": "1",
                "type": [{"code": "CodeableConcept"}],
                "binding": {
                    "strength": "required",
                    "valueSet": "http://example.org/ValueSet/ObservationCodes"
                }
            }, {
                "id": "Observation.valueString",
                "path": "Observation.valueString",
                "min": 0,
                "max": "1",
                "type": [{"code": "string"}]
            }]
        }
    }"#;

    // IHPII Identifier profile (profiled Identifier type)
    let ihpii_identifier_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/IHPII-Identifier",
        "name": "IHPIIIdentifier",
        "type": "Identifier",
        "kind": "complex-type",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Identifier",
                "path": "Identifier",
                "min": 0,
                "max": "*"
            }, {
                "id": "Identifier.system",
                "path": "Identifier.system",
                "min": 1,
                "max": "1",
                "type": [{"code": "uri"}],
                "fixedUri": "http://ns.electronichealth.net.au/id/hpii/1.0"
            }, {
                "id": "Identifier.value",
                "path": "Identifier.value",
                "min": 1,
                "max": "1",
                "type": [{"code": "string"}]
            }]
        }
    }"#;

    // BirthSex extension definition
    let birth_sex_extension_sd_json = r#"{
        "resourceType": "StructureDefinition",
        "url": "http://example.org/StructureDefinition/BirthSexExtension",
        "name": "BirthSexExtension",
        "type": "Extension",
        "kind": "complex-type",
        "derivation": "constraint",
        "snapshot": {
            "element": [{
                "id": "Extension",
                "path": "Extension",
                "min": 0,
                "max": "*"
            }, {
                "id": "Extension.url",
                "path": "Extension.url",
                "min": 1,
                "max": "1",
                "type": [{"code": "uri"}],
                "fixedUri": "http://example.org/StructureDefinition/birth-sex"
            }, {
                "id": "Extension.value[x]",
                "path": "Extension.value[x]",
                "min": 1,
                "max": "1",
                "type": [{"code": "CodeableConcept"}],
                "binding": {
                    "strength": "required",
                    "valueSet": "http://example.org/ValueSet/BirthSex"
                }
            }]
        }
    }"#;

    // ValueSet for Observation.status binding
    let observation_status_vs_json = r#"{
        "resourceType": "ValueSet",
        "url": "http://example.org/ValueSet/ObservationStatus",
        "name": "ObservationStatus",
        "status": "active",
        "compose": {
            "include": [{
                "system": "http://hl7.org/fhir/observation-status"
            }]
        }
    }"#;

    // ValueSet for Observation.code binding
    let observation_codes_vs_json = r#"{
        "resourceType": "ValueSet",
        "url": "http://example.org/ValueSet/ObservationCodes",
        "name": "ObservationCodes",
        "status": "active",
        "compose": {
            "include": [{
                "system": "http://loinc.org"
            }]
        }
    }"#;

    // ValueSet for BirthSex binding
    let birth_sex_vs_json = r#"{
        "resourceType": "ValueSet",
        "url": "http://example.org/ValueSet/BirthSex",
        "name": "BirthSex",
        "status": "active",
        "compose": {
            "include": [{
                "system": "http://hl7.org/fhir/administrative-gender"
            }]
        }
    }"#;

    // Composite SearchParameter for Patient
    let composite_sp_json = r#"{
        "resourceType": "SearchParameter",
        "url": "http://example.org/SearchParameter/patient-name-birthdate",
        "name": "name-birthdate",
        "code": "name-birthdate",
        "base": ["Patient"],
        "type": "composite",
        "expression": "Patient.name | Patient.birthDate",
        "component": [
            {"definition": "http://hl7.org/fhir/SearchParameter/Patient-name", "expression": "Patient.name"},
            {"definition": "http://hl7.org/fhir/SearchParameter/Patient-birthdate", "expression": "Patient.birthDate"}
        ]
    }"#;

    let mut tar_data = Vec::new();
    {
        let mut tar = tar::Builder::new(&mut tar_data);

        let files = [
            ("package/CapabilityStatement-extended.json", cs_json),
            (
                "package/StructureDefinition-ExtendedPatient.json",
                extended_patient_sd_json,
            ),
            (
                "package/StructureDefinition-BasePatient.json",
                base_patient_sd_json,
            ),
            (
                "package/StructureDefinition-ExtendedObservation.json",
                extended_observation_sd_json,
            ),
            (
                "package/StructureDefinition-IHPII-Identifier.json",
                ihpii_identifier_sd_json,
            ),
            (
                "package/StructureDefinition-BirthSexExtension.json",
                birth_sex_extension_sd_json,
            ),
            (
                "package/ValueSet-ObservationStatus.json",
                observation_status_vs_json,
            ),
            (
                "package/ValueSet-ObservationCodes.json",
                observation_codes_vs_json,
            ),
            ("package/ValueSet-BirthSex.json", birth_sex_vs_json),
            (
                "package/SearchParameter-patient-name-birthdate.json",
                composite_sp_json,
            ),
        ];

        for (path, content) in &files {
            let mut header = tar::Header::new_gnu();
            header.set_path(path).unwrap();
            header.set_size(content.len() as u64);
            header.set_cksum();
            tar.append_data(&mut header, *path, content.as_bytes())
                .unwrap();
        }

        tar.finish().unwrap();
    }

    let mut gz_data = Vec::new();
    {
        let mut gz = flate2::write::GzEncoder::new(&mut gz_data, flate2::Compression::default());
        gz.write_all(&tar_data).unwrap();
        gz.finish().unwrap();
    }

    gz_data
}
