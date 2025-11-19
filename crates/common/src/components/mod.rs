use std::{
    hash::{
        Hash,
        Hasher,
    },
    str::FromStr,
};

use value::{
    DeveloperDocumentId,
    TableNamespace,
};

/// Derive a deterministic child seed from a parent seed and child component name.
///
/// This ensures that child components have reproducible random behavior that
/// depends on their parent's seed, making the entire component tree deterministic
/// from a single root seed.
///
/// # Arguments
/// * `parent_seed` - The 32-byte seed of the parent component
/// * `child_name` - The name of the child component
///
/// # Returns
/// A new 32-byte seed for the child component
pub fn derive_child_seed(parent_seed: &[u8; 32], child_name: &ComponentName) -> [u8; 32] {
    use std::collections::hash_map::DefaultHasher;

    let mut hasher = DefaultHasher::new();

    // Include parent seed
    parent_seed.hash(&mut hasher);

    // Include child name for uniqueness
    child_name.as_str().hash(&mut hasher);

    // Include a domain separator
    "convex-child-component-seed".hash(&mut hasher);

    let hash = hasher.finish();

    // Expand the 64-bit hash to a full 32-byte seed
    // We use a simple expansion method that's sufficient for determinism
    let mut result = [0u8; 32];
    let hash_bytes = hash.to_le_bytes();

    // Copy the hash multiple times to fill the seed
    for (i, byte) in result.iter_mut().enumerate() {
        *byte = hash_bytes[i % 8];
        // Mix in the position to add more entropy
        *byte = byte.wrapping_add(i as u8);
    }

    result
}

mod component_definition_path;
mod component_path;
mod function_paths;
mod module_paths;
mod reference;
mod resource;

pub use self::{
    component_definition_path::ComponentDefinitionPath,
    component_path::{
        ComponentName,
        ComponentPath,
    },
    function_paths::{
        CanonicalizedComponentFunctionPath,
        ComponentDefinitionFunctionPath,
        ComponentFunctionPath,
        ExportPath,
        PublicFunctionPath,
        ResolvedComponentFunctionPath,
    },
    module_paths::CanonicalizedComponentModulePath,
    reference::Reference,
    resource::{
        Resource,
        SerializedResource,
    },
};

// Globally unique system-assigned ID for a component.
#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ComponentId {
    Root,
    Child(DeveloperDocumentId),
}

impl ComponentId {
    pub fn new(is_root: bool, id: DeveloperDocumentId) -> Self {
        if is_root {
            ComponentId::Root
        } else {
            ComponentId::Child(id)
        }
    }

    pub fn is_root(&self) -> bool {
        matches!(self, ComponentId::Root)
    }

    /// Component for tests where we need a user component.
    /// Ideally we could switch this to some other component with no test
    /// breakage.
    #[cfg(any(test, feature = "testing"))]
    pub const fn test_user() -> Self {
        ComponentId::Root
    }

    pub fn serialize_to_string(&self) -> Option<String> {
        match self {
            ComponentId::Root => None,
            ComponentId::Child(id) => Some(id.to_string()),
        }
    }

    pub fn deserialize_from_string(s: Option<&str>) -> anyhow::Result<Self> {
        match s {
            None => Ok(ComponentId::Root),
            Some(s) => Ok(ComponentId::Child(DeveloperDocumentId::from_str(s)?)),
        }
    }
}

impl From<ComponentId> for TableNamespace {
    fn from(value: ComponentId) -> Self {
        match value {
            ComponentId::Root => TableNamespace::root_component(),
            ComponentId::Child(id) => TableNamespace::ByComponent(id),
        }
    }
}

impl From<TableNamespace> for ComponentId {
    fn from(value: TableNamespace) -> Self {
        match value {
            TableNamespace::Global => ComponentId::Root,
            TableNamespace::ByComponent(id) => ComponentId::Child(id),
        }
    }
}

#[cfg(any(test, feature = "testing"))]
mod proptests {
    use proptest::prelude::*;

    use super::{
        ComponentDefinitionId,
        ComponentId,
    };

    impl Arbitrary for ComponentId {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            Just(ComponentId::Root).boxed()
        }
    }

    impl Arbitrary for ComponentDefinitionId {
        type Parameters = ();
        type Strategy = BoxedStrategy<Self>;

        fn arbitrary_with(_args: Self::Parameters) -> Self::Strategy {
            Just(ComponentDefinitionId::Root).boxed()
        }
    }
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub enum ComponentDefinitionId {
    Root,
    Child(DeveloperDocumentId),
}

impl ComponentDefinitionId {
    pub fn is_root(&self) -> bool {
        matches!(self, ComponentDefinitionId::Root)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_derive_child_seed_deterministic() {
        let parent_seed = [42u8; 32];
        let child_name: ComponentName = "auth".parse().unwrap();

        let seed1 = derive_child_seed(&parent_seed, &child_name);
        let seed2 = derive_child_seed(&parent_seed, &child_name);

        // Same inputs should produce same output
        assert_eq!(seed1, seed2);
    }

    #[test]
    fn test_derive_child_seed_different_children() {
        let parent_seed = [42u8; 32];
        let child1: ComponentName = "auth".parse().unwrap();
        let child2: ComponentName = "payments".parse().unwrap();

        let seed1 = derive_child_seed(&parent_seed, &child1);
        let seed2 = derive_child_seed(&parent_seed, &child2);

        // Different children should get different seeds
        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_derive_child_seed_different_parents() {
        let parent1 = [1u8; 32];
        let parent2 = [2u8; 32];
        let child_name: ComponentName = "auth".parse().unwrap();

        let seed1 = derive_child_seed(&parent1, &child_name);
        let seed2 = derive_child_seed(&parent2, &child_name);

        // Different parent seeds should produce different child seeds
        assert_ne!(seed1, seed2);
    }

    #[test]
    fn test_derive_child_seed_chain() {
        // Test that we can chain seed derivation for nested components
        let root_seed = [0u8; 32];
        let child1: ComponentName = "dashboard".parse().unwrap();
        let child2: ComponentName = "analytics".parse().unwrap();

        let dashboard_seed = derive_child_seed(&root_seed, &child1);
        let analytics_seed = derive_child_seed(&dashboard_seed, &child2);

        // Should be deterministic
        let dashboard_seed2 = derive_child_seed(&root_seed, &child1);
        let analytics_seed2 = derive_child_seed(&dashboard_seed2, &child2);

        assert_eq!(analytics_seed, analytics_seed2);
    }
}
