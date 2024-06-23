//! Zarr metadata.
//!
//! [`ArrayMetadata`] and [`GroupMetadata`] can hold any conformant array/group metadata.
//!
//! All known array metadata is defined in this module, even if `zarrs` has not been compiled with the appropriate flags to use it.
//! An exception is the configuration of experimental codecs, which are feature gated.

mod array;
mod group;

/// Zarr V3 metadata.
pub mod v3;

/// Zarr V2 metadata.
pub mod v2;

pub use array::{array_metadata_v2_to_v3, ArrayMetadata, ArrayMetadataV2ToV3ConversionError};
pub use group::{group_metadata_v2_to_v3, GroupMetadata};
pub use v2::{ArrayMetadataV2, GroupMetadataV2, MetadataV2};
pub use v3::{
    AdditionalFields, ArrayMetadataV3, ConfigurationInvalidError, GroupMetadataV3, MetadataV3,
    UnsupportedAdditionalFieldError,
};

use crate::config::global_config;

/// A type alias for [`MetadataV3`].
///
/// Kept for backwards compatibility with `zarrs` < 0.15.
pub type Metadata = MetadataV3;

/// The metadata version to retrieve.
///
/// Used with [`crate::array::Array::open_opt`], [`crate::group::Group::open_opt`].
pub enum MetadataRetrieveVersion {
    /// Either V3 or V2. V3 is prioritised over V2 if found.
    Default,
    /// V3
    V3,
    /// V2
    V2,
}

/// Version options for [`Array::store_metadata`](crate::array::Array::store_metadata) and [`Group::store_metadata`](crate::group::Group::store_metadata), and their async variants.
#[derive(Debug, Clone, Copy)]
pub enum MetadataConvertVersion {
    /// Write the same version as the input metadata.
    Default,
    /// Write Zarr V3 metadata. Zarr V2 metadata will not be automatically removed if it exists.
    V3,
}

impl Default for MetadataConvertVersion {
    fn default() -> Self {
        *global_config().metadata_convert_version()
    }
}

/// Version options for [`Array::erase_metadata`](crate::array::Array::erase_metadata) and [`Group::erase_metadata`](crate::group::Group::erase_metadata), and their async variants.
#[derive(Debug, Clone, Copy)]
pub enum MetadataEraseVersion {
    /// Erase the same version as the input metadata.
    Default,
    /// Erase all metadata.
    All,
    /// Erase V3 metadata.
    V3,
    /// Erase V2 metadata.
    V2,
}

impl Default for MetadataEraseVersion {
    fn default() -> Self {
        *global_config().metadata_erase_version()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use v3::{AdditionalFields, MetadataV3};

    #[test]
    fn metadata() {
        let metadata = MetadataV3::try_from(r#""bytes""#);
        assert!(metadata.is_ok());
        assert_eq!(metadata.unwrap().to_string(), r#"bytes"#);
        assert!(MetadataV3::try_from(r#"{ "name": "bytes" }"#).is_ok());
        let metadata =
            MetadataV3::try_from(r#"{ "name": "bytes", "configuration": { "endian": "little" } }"#);
        assert!(metadata.is_ok());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.to_string(), r#"bytes {"endian":"little"}"#);
        assert_eq!(metadata.name(), "bytes");
        assert!(metadata.configuration().is_some());
        let configuration = metadata.configuration().unwrap();
        assert!(configuration.contains_key("endian"));
        assert_eq!(
            configuration.get("endian").unwrap().as_str().unwrap(),
            "little"
        );
        assert_eq!(
            MetadataV3::try_from(r#"{ "name": "bytes", "invalid": { "endian": "little" } }"#)
                .unwrap_err()
                .to_string(),
            r#"Expected metadata "<name>" or {"name":"<name>"} or {"name":"<name>","configuration":{}}"#
        );
        let metadata =
            MetadataV3::try_from(r#"{ "name": "bytes", "configuration": { "endian": "little" } }"#)
                .unwrap();
        let mut configuration = serde_json::Map::new();
        configuration.insert("endian".to_string(), "little".into());
        assert_eq!(metadata.configuration(), Some(&configuration));
    }

    #[test]
    fn additional_fields_valid() {
        let mut additional_fields_map = serde_json::Map::new();
        let mut additional_field = serde_json::Map::new();
        additional_field.insert("must_understand".to_string(), false.into());
        additional_fields_map.insert("key".to_string(), additional_field.into());
        let additional_fields: AdditionalFields = additional_fields_map.clone().into();
        assert!(additional_fields.validate().is_ok());
        assert_eq!(additional_fields.as_map(), &additional_fields_map);
    }

    #[test]
    fn additional_fields_invalid1() {
        let mut additional_fields = serde_json::Map::new();
        let mut additional_field = serde_json::Map::new();
        additional_field.insert("must_understand".to_string(), true.into());
        additional_fields.insert("key".to_string(), additional_field.clone().into());
        let additional_fields: AdditionalFields = additional_fields.into();
        let validate = additional_fields.validate();
        assert!(validate.is_err());
        let err = validate.unwrap_err();
        assert_eq!(err.name(), "key");
        assert_eq!(err.value(), &serde_json::Value::Object(additional_field));
    }

    #[test]
    fn additional_fields_invalid2() {
        let mut additional_fields = serde_json::Map::new();
        let additional_field = serde_json::Map::new();
        additional_fields.insert("key".to_string(), additional_field.into());
        let additional_fields: AdditionalFields = additional_fields.into();
        assert!(additional_fields.validate().is_err());
    }

    #[test]
    fn additional_fields_invalid3() {
        let mut additional_fields = serde_json::Map::new();
        let mut additional_field = serde_json::Map::new();
        additional_field.insert("must_understand".to_string(), 0.into());
        additional_fields.insert("key".to_string(), additional_field.into());
        let additional_fields: AdditionalFields = additional_fields.into();
        assert!(additional_fields.validate().is_err());
    }
}
