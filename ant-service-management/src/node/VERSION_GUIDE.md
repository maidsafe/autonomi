# NodeServiceData Version Guide

This document provides comprehensive guidance for managing NodeServiceData versions, including when to create new versions, how to implement custom deserialization, and how to maintain proper testing.

## Overview

The NodeServiceData structure uses a versioned schema approach to handle backward compatibility and data migration. Currently, we have:

- **V0**: Original structure (no schema_version field)
- **V1**: Added schema_version, renamed some fields (`peers_args` → `initial_peers_config`, `upnp` → `no_upnp`, `home_network` → `relay`)
- **V2**: Added `alpha` field (current latest version)

## When Do You Need a New Version?

1. **You DON'T need a new version** if you're deprecating/removing a field. This is confirmed by the test `fields_can_be_removed_without_extra_logic`
2. **You DON'T need a new version** if you're adding a new field with `#[serde(default)]`. This is confirmed by the test `fields_can_be_added_without_extra_logic_with_serde_default`. But if you want to be super sure, creating a new version is fine here as seen with `node_service_data_v2.rs`

**You DO need a new version** if your change doesn't fall within these two categories - such as renaming a field or changing field logic/behavior or even adding a new field (with/without `#[serde(default)]`)

## Custom Deserialization Implementation

### Current Implementation Pattern

The current version (V2) uses a custom deserialization approach instead of deriving `Deserialize`.

```rust
impl<'de> Deserialize<'de> for NodeServiceData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Check schema_version and route to appropriate deserializer
        match schema_version {
            Some(2) => {
                // Use custom deserialize_v2() instead of derived Deserialize
                match super::node_service_data_v2::NodeServiceDataV2::deserialize_v2(
                    &mut serde_json::de::Deserializer::from_str(&json_value.to_string()),
                ) {
                    Ok(v2) => Ok(v2),
                    Err(e) => Err(D::Error::custom(format!("Failed to deserialize as V2: {}", e))),
                }
            }
            // ... handle other versions
        }
    }
}
```

### Key Points About Custom Deserialization

1. **Latest version uses custom method**: The current latest version (V2) uses `deserialize_v2()` instead of deriving `Deserialize`
2. **Older versions use derived Deserialize**: V0 and V1 use standard `serde::from_value()` with derived `Deserialize`
3. **Version routing**: The custom implementation checks `schema_version` and routes to the appropriate deserialization method

## Adding a New Version (e.g., V3)

When you need to add a new version, follow this pattern:

### Step 1: Create the new version file (e.g., `node_service_data_v3.rs`)

```rust
pub const NODE_SERVICE_DATA_SCHEMA_V3: u32 = 3;

#[derive(Clone, Debug, Serialize)]
pub struct NodeServiceDataV3 {
    // Add your new fields here
    pub new_field: SomeType,
    // ... existing fields
}

impl NodeServiceDataV3 {
    pub fn deserialize_v3<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Define helper struct with Deserialize derived
        #[derive(Deserialize)]
        struct NodeServiceDataV3Helper {
            // All fields with proper serde attributes
        }
        
        let helper = NodeServiceDataV3Helper::deserialize(deserializer)?;
        
        Ok(Self {
            // Map all fields from helper
        })
    }
}
```

### Step 2: Update `node_service_data.rs`

```rust
// Update type alias to point to new version
pub type NodeServiceData = super::node_service_data_v3::NodeServiceDataV3;
pub const NODE_SERVICE_DATA_SCHEMA_LATEST: u32 = 
    super::node_service_data_v3::NODE_SERVICE_DATA_SCHEMA_V3;

// Update custom deserialize implementation
impl<'de> Deserialize<'de> for NodeServiceData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error> {
        match schema_version {
            Some(3) => {
                // New version uses custom deserialize_v3
                match super::node_service_data_v3::NodeServiceDataV3::deserialize_v3(
                    &mut serde_json::de::Deserializer::from_str(&json_value.to_string()),
                ) {
                    Ok(v3) => Ok(v3),
                    Err(e) => Err(D::Error::custom(format!("Failed to deserialize as V3: {}", e))),
                }
            }
            Some(2) => {
                // Previous latest version now uses standard derived Deserialize
                match serde_json::from_value::<super::node_service_data_v2::NodeServiceDataV2>(
                    json_value,
                ) {
                    Ok(v2) => {
                        let v3: super::node_service_data_v3::NodeServiceDataV3 = v2.into();
                        Ok(v3)
                    }
                    Err(e) => Err(D::Error::custom(format!("Failed to deserialize as V2: {}", e))),
                }
            }
            // ... other versions
        }
    }
}
```

### Step 3: Update the previous latest version

Remove the custom `deserialize_v2` method and add `Deserialize` to the derive macro:

```rust
// In node_service_data_v2.rs
#[derive(Clone, Debug, Serialize, Deserialize)]  // Add Deserialize here
pub struct NodeServiceDataV2 {
    // ... fields
}

// Remove the impl NodeServiceDataV2 block with deserialize_v2
```

### Step 4: Implement conversion from previous version

```rust
// In node_service_data_v3.rs
impl From<super::node_service_data_v2::NodeServiceDataV2> for NodeServiceDataV3 {
    fn from(v2: super::node_service_data_v2::NodeServiceDataV2) -> Self {
        Self {
            new_field: Default::default(), // or appropriate transformation
            // ... map all existing fields
        }
    }
}
```

## Testing Strategy

### Types of Tests Currently Implemented

#### 1. Version-to-Version Migration Tests

**Location**: Each version file contains tests for migrating to the next version

**Example** (`node_service_data_v0.rs:169`):
```rust
#[test]
fn test_v0_to_v1_conversion() {
    let v0_data = NodeServiceDataV0 { /* ... */ };
    let v1: NodeServiceDataV1 = v0_data.into();
    
    // Check field transformations
    assert!(v1.no_upnp); // V0 upnp: false → V1 no_upnp: true
    assert!(v1.relay);   // V0 home_network: true → V1 relay: true
}
```

**What they test**:
- Field renames and transformations
- Boolean logic inversions
- Default value assignments for new fields

#### 2. Version-to-Latest Migration Tests

**Location**: Each version file contains tests for migrating directly to the latest version

**Example** (`node_service_data_v0.rs:121`):
```rust
#[test]  
fn test_v0_conversion_to_latest() {
    let v0_data = NodeServiceDataV0 { /* ... */ };
    let v0_json = serde_json::to_value(&v0_data).unwrap();
    let latest: NodeServiceData = serde_json::from_value(v0_json).unwrap();
    
    assert_eq!(latest.schema_version, NODE_SERVICE_DATA_SCHEMA_LATEST);
}
```

**What they test**:
- Complete migration chain works end-to-end
- Latest version has correct schema_version
- No data corruption during full migration
