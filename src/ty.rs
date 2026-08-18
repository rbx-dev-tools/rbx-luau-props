//! The Roblox value types the UI surface is built from, and their Luau spelling.
//!
//! The mapping is exhaustive rather than defaulting: a type this table does not
//! name is an error, not an `any`. A generated file that silently widens one
//! property to `any` is worse than a build that stops, because the widening is
//! invisible at the call site and survives every later regeneration.

use anyhow::{bail, Result};

use crate::dump::ValueType;

/// The Luau type written for a property of this value type.
pub fn luau_type(value_type: &ValueType) -> Result<String> {
    let name = value_type.name.as_str();

    match value_type.category.as_str() {
        "Enum" => return Ok(format!("Enum.{name}")),

        // A property holding a reference to another instance. The dump records
        // the target class, so `Adornee: Instance` can be `Adornee: Camera`
        // where the engine is that specific.
        "Class" => return Ok(name.to_owned()),

        "Primitive" => {
            let luau = match name {
                "bool" => "boolean",
                // Luau has one number type; the engine's width is neither
                // expressible nor useful in a prop table.
                "int" | "int64" | "float" | "double" => "number",
                "string" => "string",
                other => bail!("unmodelled primitive: {other}"),
            };
            return Ok(luau.to_owned());
        }

        "DataType" => {}
        other => bail!("unmodelled value category: {other}"),
    }

    let luau = match name {
        // Asset references are plain strings at the Luau boundary
        // (`rbxassetid://...`), unlike the newer Content userdata.
        "ContentId" | "BinaryString" | "ProtectedString" => "string",

        // Everything else in this category is a userdata whose Luau name is
        // already the dump's name. Listing them rather than passing them
        // through keeps an unknown type an error.
        "BrickColor"
        | "CFrame"
        | "Color3"
        | "ColorSequence"
        | "Content"
        | "Font"
        | "NumberRange"
        | "NumberSequence"
        | "Rect"
        | "SecurityCapabilities"
        | "UDim"
        | "UDim2"
        | "Vector2"
        | "Vector3" => name,

        other => bail!("unmodelled data type: {other}"),
    };

    Ok(luau.to_owned())
}
