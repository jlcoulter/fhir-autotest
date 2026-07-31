use serde::{Deserialize, Serialize};

/// FHIR R4 CapabilityStatement resource.
/// Describes what a FHIR server supports: resources, interactions, search params, operations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityStatement {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub url: Option<String>,
    pub name: Option<String>,
    pub status: Option<String>,
    #[serde(default)]
    pub software: Option<Software>,
    #[serde(default)]
    pub implementation: Option<Implementation>,
    #[serde(default)]
    pub messaging: Vec<Messaging>,
    #[serde(default)]
    pub document: Vec<Document>,
    #[serde(default)]
    pub rest: Vec<Rest>,
}

/// Software information for a CapabilityStatement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Software {
    pub name: Option<String>,
    pub version: Option<String>,
    #[serde(rename = "releaseDate")]
    pub release_date: Option<String>,
}

/// Implementation information for a CapabilityStatement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Implementation {
    pub description: Option<String>,
    pub url: Option<String>,
    #[serde(rename = "custodian")]
    pub custodian: Option<serde_json::Value>,
}

/// Messaging capabilities declared in a CapabilityStatement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Messaging {
    pub endpoint: Option<String>,
    #[serde(rename = "reliableCache")]
    pub reliable_cache: Option<u32>,
    #[serde(default)]
    pub documentation: Option<String>,
    #[serde(rename = "supportedMessage", default)]
    pub supported_message: Vec<MessagingSupportedMessage>,
}

/// A supported message type within a messaging capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessagingSupportedMessage {
    pub mode: Option<String>,
    pub definition: Option<String>,
}

/// Document capabilities declared in a CapabilityStatement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub mode: Option<String>,
    pub documentation: Option<String>,
    pub profile: Option<String>,
}

/// Security declarations for a REST endpoint.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Security {
    #[serde(default)]
    pub cors: Option<bool>,
    #[serde(default)]
    pub service: Vec<SecurityService>,
    pub description: Option<String>,
}

/// A security service (e.g., OAuth) declared in a CapabilityStatement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityService {
    pub coding: Option<SecurityServiceCoding>,
    pub text: Option<String>,
}

/// Coding for a security service.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityServiceCoding {
    pub system: Option<String>,
    pub code: Option<String>,
    pub display: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rest {
    pub mode: String,
    #[serde(default)]
    pub resource: Vec<RestResource>,
    #[serde(default)]
    pub interaction: Vec<RestInteraction>,
    #[serde(default)]
    pub operation: Vec<RestOperation>,
    #[serde(default)]
    pub security: Option<Security>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestResource {
    #[serde(rename = "type")]
    pub resource_type: String,
    pub profile: Option<String>,
    #[serde(rename = "supportedProfile", default)]
    pub supported_profile: Vec<String>,
    #[serde(default)]
    pub interaction: Vec<RestInteraction>,
    #[serde(rename = "searchParam", default)]
    pub search_param: Vec<RestSearchParam>,
    #[serde(default)]
    pub operation: Vec<RestOperation>,
    #[serde(rename = "readHistory", default)]
    pub read_history: Option<bool>,
    #[serde(rename = "updateCreate", default)]
    pub update_create: Option<bool>,
    #[serde(default)]
    pub versioning: Option<String>,
    #[serde(rename = "conditionalCreate", default)]
    pub conditional_create: Option<bool>,
    #[serde(rename = "conditionalRead", default)]
    pub conditional_read: Option<String>,
    #[serde(rename = "conditionalUpdate", default)]
    pub conditional_update: Option<bool>,
    #[serde(rename = "conditionalDelete", default)]
    pub conditional_delete: Option<String>,
    #[serde(rename = "searchInclude", default)]
    pub search_include: Vec<String>,
    #[serde(rename = "searchRevInclude", default)]
    pub search_revinclude: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestInteraction {
    pub code: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestSearchParam {
    pub name: String,
    #[serde(rename = "type")]
    pub param_type: String,
    pub definition: Option<String>,
    pub documentation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RestOperation {
    pub name: String,
    pub definition: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserialize_minimal_capability_statement() {
        let json = r#"{
            "resourceType": "CapabilityStatement",
            "status": "active",
            "name": "test",
            "rest": [{
                "mode": "server",
                "resource": [{
                    "type": "Patient",
                    "interaction": [{"code": "read"}, {"code": "search-type"}],
                    "searchParam": [{"name": "name", "type": "string"}]
                }],
                "interaction": []
            }]
        }"#;
        let cs: CapabilityStatement = serde_json::from_str(json).unwrap();
        assert_eq!(cs.rest[0].resource[0].resource_type, "Patient");
        assert_eq!(cs.rest[0].resource[0].interaction.len(), 2);
        assert_eq!(cs.rest[0].resource[0].search_param[0].name, "name");
        assert!(cs.software.is_none());
        assert!(cs.implementation.is_none());
        assert!(cs.messaging.is_empty());
        assert!(cs.document.is_empty());
        assert!(cs.rest[0].security.is_none());
    }

    #[test]
    fn deserialize_capability_statement_with_all_fields() {
        let json = r#"{
            "resourceType": "CapabilityStatement",
            "status": "active",
            "name": "FullCS",
            "software": {
                "name": "HAPI FHIR",
                "version": "6.2.0",
                "releaseDate": "2023-01-01"
            },
            "implementation": {
                "description": "Test FHIR Server",
                "url": "http://example.org/fhir"
            },
            "messaging": [{
                "endpoint": "http://example.org/messaging",
                "reliableCache": 60,
                "supportedMessage": [{
                    "mode": "sender",
                    "definition": "http://hl7.org/fhir/MessageDefinition/test"
                }]
            }],
            "document": [{
                "mode": "producer",
                "profile": "http://hl7.org/fhir/StructureDefinition/Bundle"
            }],
            "rest": [{
                "mode": "server",
                "security": {
                    "cors": true,
                    "service": [{
                        "coding": {
                            "system": "http://hl7.org/fhir/restful-security-service",
                            "code": "OAuth",
                            "display": "OAuth"
                        }
                    }],
                    "description": "OAuth2 with SMART on FHIR"
                },
                "resource": [{
                    "type": "Patient",
                    "interaction": [{"code": "read"}]
                }],
                "interaction": [{"code": "batch"}, {"code": "transaction"}]
            }]
        }"#;
        let cs: CapabilityStatement = serde_json::from_str(json).unwrap();
        assert_eq!(cs.resource_type, "CapabilityStatement");
        assert_eq!(cs.name.as_deref(), Some("FullCS"));

        // Software
        let sw = cs.software.as_ref().unwrap();
        assert_eq!(sw.name.as_deref(), Some("HAPI FHIR"));
        assert_eq!(sw.version.as_deref(), Some("6.2.0"));

        // Implementation
        let imp = cs.implementation.as_ref().unwrap();
        assert_eq!(imp.description.as_deref(), Some("Test FHIR Server"));

        // Messaging
        assert_eq!(cs.messaging.len(), 1);
        assert_eq!(
            cs.messaging[0].endpoint.as_deref(),
            Some("http://example.org/messaging")
        );
        assert_eq!(cs.messaging[0].supported_message.len(), 1);

        // Document
        assert_eq!(cs.document.len(), 1);
        assert_eq!(cs.document[0].mode.as_deref(), Some("producer"));

        // Security
        let sec = cs.rest[0].security.as_ref().unwrap();
        assert_eq!(sec.cors, Some(true));
        assert_eq!(sec.service.len(), 1);
        assert_eq!(
            sec.service[0]
                .coding
                .as_ref()
                .and_then(|c| c.code.as_deref()),
            Some("OAuth")
        );

        // System-level interactions
        assert_eq!(cs.rest[0].interaction.len(), 2);
        assert_eq!(cs.rest[0].interaction[0].code, "batch");
        assert_eq!(cs.rest[0].interaction[1].code, "transaction");
    }
}
