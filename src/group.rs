//! Zarr groups.
//!
//! A Zarr group is a node in a Zarr hierarchy.
//! It can have associated metadata and may have child nodes (groups or [`arrays`](crate::array)).
//! See <https://zarr-specs.readthedocs.io/en/latest/v3/core/v3.0.html#group>.
//!
//! Use [`GroupBuilder`] to setup a new group, or use [`Group::new`] to read and/or write an existing group.
//!
//! A group can optionally store attributes in metadata in an accompanying `zarr.json` file. For example:
//! ```json
//! {
//!     "zarr_format": 3,
//!     "node_type": "group",
//!     "attributes": {
//!         "spam": "ham",
//!         "eggs": 42,
//!     }
//! }
//! ```
//! See <https://zarr-specs.readthedocs.io/en/latest/v3/core/v3.0.html#group-metadata> for more information on group metadata.

mod group_builder;
mod group_metadata_options;

use std::sync::Arc;

use derive_more::Display;
use thiserror::Error;

use crate::{
    config::{MetadataOptionsEraseVersion, MetadataOptionsStoreVersion},
    metadata::{
        group_metadata_v2_to_v3,
        v3::{AdditionalFields, UnsupportedAdditionalFieldError},
    },
    node::{NodePath, NodePathError},
    storage::{
        meta_key, meta_key_v2_attributes, meta_key_v2_group, ReadableStorageTraits, StorageError,
        StorageHandle, WritableStorageTraits,
    },
};

#[cfg(feature = "async")]
use crate::storage::{AsyncReadableStorageTraits, AsyncWritableStorageTraits};

pub use self::group_builder::GroupBuilder;
pub use crate::metadata::{v3::GroupMetadataV3, GroupMetadata};
pub use group_metadata_options::GroupMetadataOptions;

/// A group.
#[derive(Clone, Debug, Display)]
#[display(
    fmt = "group at {path} with metadata {}",
    "serde_json::to_string(metadata).unwrap_or_default()"
)]
pub struct Group<TStorage: ?Sized> {
    /// The storage.
    #[allow(dead_code)]
    storage: Arc<TStorage>,
    /// The path of the group in the store.
    #[allow(dead_code)]
    path: NodePath,
    /// The metadata.
    metadata: GroupMetadata,
}

impl<TStorage: ?Sized> Group<TStorage> {
    /// Create a group in `storage` at `path` with `metadata`.
    /// This does **not** write to the store, use [`store_metadata`](Group<WritableStorageTraits>::store_metadata) to write `metadata` to `storage`.
    ///
    /// # Errors
    ///
    /// Returns [`GroupCreateError`] if any metadata is invalid.
    pub fn new_with_metadata(
        storage: Arc<TStorage>,
        path: &str,
        metadata: GroupMetadata,
    ) -> Result<Self, GroupCreateError> {
        let path = NodePath::new(path)?;
        validate_group_metadata(&metadata)?;
        Ok(Self {
            storage,
            path,
            metadata,
        })
    }

    /// Get path.
    #[must_use]
    pub const fn path(&self) -> &NodePath {
        &self.path
    }

    /// Get attributes.
    #[must_use]
    pub const fn attributes(&self) -> &serde_json::Map<String, serde_json::Value> {
        match &self.metadata {
            GroupMetadata::V3(metadata) => &metadata.attributes,
            GroupMetadata::V2(metadata) => &metadata.attributes,
        }
    }

    /// Get additional fields.
    #[must_use]
    pub const fn additional_fields(&self) -> &AdditionalFields {
        match &self.metadata {
            GroupMetadata::V3(metadata) => &metadata.additional_fields,
            GroupMetadata::V2(metadata) => &metadata.additional_fields,
        }
    }

    /// Get metadata.
    #[must_use]
    pub fn metadata(&self) -> GroupMetadata {
        self.metadata.clone()
    }

    /// Mutably borrow the group attributes.
    #[must_use]
    pub fn attributes_mut(&mut self) -> &mut serde_json::Map<String, serde_json::Value> {
        match &mut self.metadata {
            GroupMetadata::V3(metadata) => &mut metadata.attributes,
            GroupMetadata::V2(metadata) => &mut metadata.attributes,
        }
    }

    /// Mutably borrow the additional fields.
    #[must_use]
    pub fn additional_fields_mut(&mut self) -> &mut AdditionalFields {
        match &mut self.metadata {
            GroupMetadata::V3(metadata) => &mut metadata.additional_fields,
            GroupMetadata::V2(metadata) => &mut metadata.additional_fields,
        }
    }
}

impl<TStorage: ?Sized + ReadableStorageTraits> Group<TStorage> {
    /// Create a group in `storage` at `path`. The metadata is read from the store.
    ///
    /// # Errors
    ///
    /// Returns [`GroupCreateError`] if there is a storage error or any metadata is invalid.
    pub fn new(storage: Arc<TStorage>, path: &str) -> Result<Self, GroupCreateError> {
        let node_path = path.try_into()?;
        let key = meta_key(&node_path);
        let metadata: GroupMetadata = match storage.get(&key)? {
            Some(metadata) => serde_json::from_slice(&metadata)
                .map_err(|err| StorageError::InvalidMetadata(key, err.to_string()))?,
            None => GroupMetadataV3::default().into(),
        };
        Self::new_with_metadata(storage, path, metadata)
    }
}

#[cfg(feature = "async")]
impl<TStorage: ?Sized + AsyncReadableStorageTraits> Group<TStorage> {
    /// Create a group in `storage` at `path`. The metadata is read from the store.
    ///
    /// # Errors
    ///
    /// Returns [`GroupCreateError`] if there is a storage error or any metadata is invalid.
    pub async fn async_new(storage: Arc<TStorage>, path: &str) -> Result<Self, GroupCreateError> {
        let node_path = path.try_into()?;
        let key = meta_key(&node_path);
        let metadata: GroupMetadata = match storage.get(&key).await? {
            Some(metadata) => serde_json::from_slice(&metadata)
                .map_err(|err| StorageError::InvalidMetadata(key, err.to_string()))?,
            None => GroupMetadataV3::default().into(),
        };
        Self::new_with_metadata(storage, path, metadata)
    }
}

/// A group creation error.
#[derive(Debug, Error)]
pub enum GroupCreateError {
    /// Invalid zarr format.
    #[error("invalid zarr format {0}, expected 3")]
    InvalidZarrFormat(usize),
    /// Invalid node type.
    #[error("invalid zarr format {0}, expected group")]
    InvalidNodeType(String),
    /// An invalid node path
    #[error(transparent)]
    NodePathError(#[from] NodePathError),
    /// Unsupported additional field.
    #[error(transparent)]
    UnsupportedAdditionalFieldError(UnsupportedAdditionalFieldError),
    /// Storage error.
    #[error(transparent)]
    StorageError(#[from] StorageError),
}

fn validate_group_metadata(metadata: &GroupMetadata) -> Result<(), GroupCreateError> {
    match metadata {
        GroupMetadata::V3(metadata) => {
            if !metadata.validate_format() {
                Err(GroupCreateError::InvalidZarrFormat(metadata.zarr_format))
            } else if !metadata.validate_node_type() {
                Err(GroupCreateError::InvalidNodeType(
                    metadata.node_type.clone(),
                ))
            } else {
                metadata
                    .additional_fields
                    .validate()
                    .map_err(GroupCreateError::UnsupportedAdditionalFieldError)
            }
        }
        GroupMetadata::V2(_) => Ok(()),
    }
}

impl<TStorage: ?Sized + ReadableStorageTraits> Group<TStorage> {}

impl<TStorage: ?Sized + WritableStorageTraits + 'static> Group<TStorage> {
    /// Store metadata.
    ///
    /// # Errors
    /// Returns [`StorageError`] if there is an underlying store error.
    pub fn store_metadata(&self) -> Result<(), StorageError> {
        let storage_handle = StorageHandle::new(self.storage.clone());
        crate::storage::create_group(&storage_handle, self.path(), &self.metadata())
    }

    /// Store metadata with non-default [`GroupMetadataOptions`].
    ///
    /// # Errors
    /// Returns [`StorageError`] if there is an underlying store error.
    pub fn store_metadata_opt(&self, options: &GroupMetadataOptions) -> Result<(), StorageError> {
        use MetadataOptionsStoreVersion as V;
        let storage_handle = Arc::new(StorageHandle::new(self.storage.clone()));

        let metadata = self.metadata();

        // Get the metadata with options applied
        // let metadata = self.metadata_opt(options);

        // Convert/store the metadata as requested
        match (metadata, options.metadata_store_version()) {
            (GroupMetadata::V3(metadata), V::Default | V::V3) => {
                // Store V3
                crate::storage::create_group(
                    &*storage_handle,
                    self.path(),
                    &GroupMetadata::V3(metadata),
                )
            }
            (GroupMetadata::V2(metadata), V::V3) => {
                // Convert V2 to V3
                let metadata = group_metadata_v2_to_v3(&metadata);
                crate::storage::create_group(
                    &*storage_handle,
                    self.path(),
                    &GroupMetadata::V3(metadata),
                )
            }
            (GroupMetadata::V2(metadata), V::Default) => {
                // Store V2
                crate::storage::create_group(
                    &*storage_handle,
                    self.path(),
                    &GroupMetadata::V2(metadata),
                )
            }
        }
    }

    /// Erase the metadata with default [`MetadataOptionsEraseVersion`] options.
    ///
    /// Succeeds if the metadata does not exist.
    ///
    /// # Errors
    /// Returns a [`StorageError`] if there is an underlying store error.
    pub fn erase_metadata(&self) -> Result<(), StorageError> {
        self.erase_metadata_opt(&MetadataOptionsEraseVersion::default())
    }

    /// Erase the metadata with non-default [`MetadataOptionsEraseVersion`] options.
    ///
    /// Succeeds if the metadata does not exist.
    ///
    /// # Errors
    /// Returns a [`StorageError`] if there is an underlying store error.
    pub fn erase_metadata_opt(
        &self,
        options: &MetadataOptionsEraseVersion,
    ) -> Result<(), StorageError> {
        let storage_handle = StorageHandle::new(self.storage.clone());
        match options {
            MetadataOptionsEraseVersion::Default => match self.metadata {
                GroupMetadata::V3(_) => storage_handle.erase(&meta_key(self.path())),
                GroupMetadata::V2(_) => {
                    storage_handle.erase(&meta_key_v2_group(self.path()))?;
                    storage_handle.erase(&meta_key_v2_attributes(self.path()))
                }
            },
            MetadataOptionsEraseVersion::All => {
                storage_handle.erase(&meta_key(self.path()))?;
                storage_handle.erase(&meta_key_v2_group(self.path()))?;
                storage_handle.erase(&meta_key_v2_attributes(self.path()))
            }
            MetadataOptionsEraseVersion::V3 => storage_handle.erase(&meta_key(self.path())),
            MetadataOptionsEraseVersion::V2 => {
                storage_handle.erase(&meta_key_v2_group(self.path()))?;
                storage_handle.erase(&meta_key_v2_attributes(self.path()))
            }
        }
    }
}

#[cfg(feature = "async")]
impl<TStorage: ?Sized + AsyncWritableStorageTraits> Group<TStorage> {
    /// Async variant of [`store_metadata`](Group::store_metadata).
    #[allow(clippy::missing_errors_doc)]
    pub async fn async_store_metadata(&self) -> Result<(), StorageError> {
        let storage_handle = StorageHandle::new(self.storage.clone());
        crate::storage::async_create_group(&storage_handle, self.path(), &self.metadata()).await
    }

    /// Async variant of [`erase_metadata`](Group::erase_metadata).
    #[allow(clippy::missing_errors_doc)]
    pub async fn async_erase_metadata(&self) -> Result<(), StorageError> {
        let storage_handle = StorageHandle::new(self.storage.clone());
        match self.metadata {
            GroupMetadata::V3(_) => storage_handle.erase(&meta_key(self.path())).await,
            GroupMetadata::V2(_) => {
                storage_handle
                    .erase(&meta_key_v2_group(self.path()))
                    .await?;
                storage_handle
                    .erase(&meta_key_v2_attributes(self.path()))
                    .await?;
                Ok(())
            }
        }
    }

    /// Async variant of [`erase_metadata_opt`](Group::erase_metadata_opt).
    #[allow(clippy::missing_errors_doc)]
    pub async fn async_erase_metadata_opt(
        &self,
        options: &MetadataOptionsEraseVersion,
    ) -> Result<(), StorageError> {
        let storage_handle = StorageHandle::new(self.storage.clone());
        match options {
            MetadataOptionsEraseVersion::Default => match self.metadata {
                GroupMetadata::V3(_) => storage_handle.erase(&meta_key(self.path())).await,
                GroupMetadata::V2(_) => {
                    storage_handle
                        .erase(&meta_key_v2_group(self.path()))
                        .await?;
                    storage_handle
                        .erase(&meta_key_v2_attributes(self.path()))
                        .await
                }
            },
            MetadataOptionsEraseVersion::All => {
                storage_handle.erase(&meta_key(self.path())).await?;
                storage_handle
                    .erase(&meta_key_v2_group(self.path()))
                    .await?;
                storage_handle
                    .erase(&meta_key_v2_attributes(self.path()))
                    .await
            }
            MetadataOptionsEraseVersion::V3 => storage_handle.erase(&meta_key(self.path())).await,
            MetadataOptionsEraseVersion::V2 => {
                storage_handle
                    .erase(&meta_key_v2_group(self.path()))
                    .await?;
                storage_handle
                    .erase(&meta_key_v2_attributes(self.path()))
                    .await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::storage::{store::MemoryStore, StoreKey};

    use super::*;

    const JSON_VALID1: &str = r#"{
    "zarr_format": 3,
    "node_type": "group",
    "attributes": {
        "spam": "ham",
        "eggs": 42
    }
}"#;

    #[test]
    fn group_metadata1() {
        let group_metadata: GroupMetadata = serde_json::from_str(JSON_VALID1).unwrap();
        let store = MemoryStore::default();
        Group::new_with_metadata(store.into(), "/", group_metadata).unwrap();
    }

    #[test]
    fn group_metadata2() {
        let group_metadata: GroupMetadata = serde_json::from_str(
            r#"{
            "zarr_format": 3,
            "node_type": "group",
            "attributes": {
                "spam": "ham",
                "eggs": 42
            },
            "unknown": {
                "must_understand": false
            }
        }"#,
        )
        .unwrap();
        let store = MemoryStore::default();
        Group::new_with_metadata(store.into(), "/", group_metadata).unwrap();
    }

    #[test]
    fn group_metadata_invalid_format() {
        let group_metadata: GroupMetadata = serde_json::from_str(
            r#"{
            "zarr_format": 2,
            "node_type": "group",
            "attributes": {
                "spam": "ham",
                "eggs": 42
            }
        }"#,
        )
        .unwrap();
        print!("{group_metadata:?}");
        let store = MemoryStore::default();
        let group_metadata = Group::new_with_metadata(store.into(), "/", group_metadata);
        assert_eq!(
            group_metadata.unwrap_err().to_string(),
            "invalid zarr format 2, expected 3"
        );
    }

    #[test]
    fn group_metadata_invalid_type() {
        let group_metadata: GroupMetadata = serde_json::from_str(
            r#"{
            "zarr_format": 3,
            "node_type": "array",
            "attributes": {
                "spam": "ham",
                "eggs": 42
            }
        }"#,
        )
        .unwrap();
        print!("{group_metadata:?}");
        let store = MemoryStore::default();
        let group_metadata = Group::new_with_metadata(store.into(), "/", group_metadata);
        assert_eq!(
            group_metadata.unwrap_err().to_string(),
            "invalid zarr format array, expected group"
        );
    }

    #[test]
    fn group_metadata_invalid_additional_field() {
        let group_metadata: GroupMetadata = serde_json::from_str(
            r#"{
                "zarr_format": 3,
                "node_type": "group",
                "attributes": {
                  "spam": "ham",
                  "eggs": 42
                },
                "unknown": "fail"
            }"#,
        )
        .unwrap();
        print!("{group_metadata:?}");
        let store = MemoryStore::default();
        let group_metadata = Group::new_with_metadata(store.into(), "/", group_metadata);
        assert_eq!(
            group_metadata.unwrap_err().to_string(),
            r#"unsupported additional field unknown with value "fail""#
        );
    }

    #[test]
    fn group_metadata_write_read() {
        let store = std::sync::Arc::new(MemoryStore::new());
        let group_path = "/group";
        let group = GroupBuilder::new()
            .build(store.clone(), group_path)
            .unwrap();
        group.store_metadata().unwrap();
        let metadata = Group::new(store, group_path).unwrap().metadata();
        assert_eq!(metadata, group.metadata());
        assert_eq!(
            group.metadata().to_string(),
            r#"{"node_type":"group","zarr_format":3}"#
        );
        assert_eq!(
            group.to_string(),
            r#"group at /group with metadata {"node_type":"group","zarr_format":3}"#
        );
    }

    #[test]
    fn group_metadata_invalid_path() {
        let group_metadata: GroupMetadata = serde_json::from_str(JSON_VALID1).unwrap();
        let store = MemoryStore::default();
        assert_eq!(
            Group::new_with_metadata(store.into(), "abc", group_metadata)
                .unwrap_err()
                .to_string(),
            "invalid node path abc"
        );
    }

    #[test]
    fn group_invalid_path() {
        let store: std::sync::Arc<MemoryStore> = std::sync::Arc::new(MemoryStore::new());
        assert_eq!(
            Group::new(store, "abc").unwrap_err().to_string(),
            "invalid node path abc"
        );
    }

    #[test]
    fn group_invalid_metadata() {
        let store: std::sync::Arc<MemoryStore> = std::sync::Arc::new(MemoryStore::new());
        store
            .set(&StoreKey::new("zarr.json").unwrap(), &[0])
            .unwrap();
        assert_eq!(
            Group::new(store, "/").unwrap_err().to_string(),
            "error parsing metadata for zarr.json: expected value at line 1 column 1"
        );
    }

    #[cfg(feature = "async")]
    #[tokio::test]
    async fn group_metadata_write_read_async() {
        let store = std::sync::Arc::new(crate::storage::store::AsyncObjectStore::new(
            object_store::memory::InMemory::new(),
        ));
        let group_path = "/group";
        let group = GroupBuilder::new()
            .build(store.clone(), group_path)
            .unwrap();
        group.async_store_metadata().await.unwrap();
        let metadata = Group::async_new(store, group_path)
            .await
            .unwrap()
            .metadata();
        assert_eq!(metadata, group.metadata());
    }

    #[test]
    fn group_default() {
        let store = std::sync::Arc::new(MemoryStore::new());
        let group_path = "/group";
        let group = Group::new(store, group_path).unwrap();
        assert_eq!(group.attributes(), &serde_json::Map::default());
        assert_eq!(group.additional_fields(), &AdditionalFields::default());
    }
}
