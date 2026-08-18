//! Tests over the vendored dump, so they assert the thing that matters: the
//! properties the engine really exposes end up in the file, and the ones it
//! refuses to let a script assign stay out of it.

use react_luau_props::{dump, emit, ir};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn surface(include_deprecated: bool) -> ir::Surface {
    let bytes = fs::read(root().join(react_luau_props::VENDORED_DUMP))
        .expect("vendored dump; run `react-luau-props fetch`");
    let parsed = dump::parse(&bytes).expect("the vendored dump parses");
    ir::build(&parsed, ir::Options { include_deprecated }).expect("the surface builds")
}

fn properties(surface: &ir::Surface) -> BTreeMap<&str, BTreeMap<&str, &str>> {
    surface
        .classes
        .iter()
        .map(|class| {
            let props = class
                .properties
                .iter()
                .map(|p| (p.name.as_str(), p.luau.as_str()))
                .collect();
            (class.name.as_str(), props)
        })
        .collect()
}

#[test]
fn covers_the_classes_a_react_tree_actually_builds() {
    let surface = surface(false);
    let props = properties(&surface);

    for class in [
        "Frame",
        "TextLabel",
        "TextButton",
        "ImageLabel",
        "ImageButton",
        "ScrollingFrame",
        "TextBox",
        "ScreenGui",
        "BillboardGui",
        "SurfaceGui",
        "CanvasGroup",
        "ViewportFrame",
        "UIListLayout",
        "UIPadding",
        "UIStroke",
        "UICorner",
    ] {
        assert!(props.contains_key(class), "{class} is missing");
    }
}

#[test]
fn leaves_out_the_classes_that_exist_only_to_be_inherited_from() {
    let built = surface(false);
    let props = properties(&built);
    for class in [
        "GuiObject",
        "GuiBase2d",
        "GuiButton",
        "UIComponent",
        "UIBase",
    ] {
        assert!(
            !props.contains_key(class),
            "{class} is abstract and should not be emitted"
        );
    }
}

// Flattening is not a formatting choice: see the note in `ir`. A composed
// shape breaks `React.Tag`, so inherited properties have to be present on the
// class itself rather than reached through an intersection.
#[test]
fn inherited_properties_are_flattened_onto_every_class() {
    let built = surface(false);
    let props = properties(&built);
    let frame = &props["Frame"];

    // Declared by GuiObject, GuiBase2d and Instance respectively.
    assert_eq!(frame.get("BackgroundColor3"), Some(&"Color3"));
    assert_eq!(frame.get("AutoLocalize"), Some(&"boolean"));
    assert_eq!(frame.get("Name"), Some(&"string"));
}

#[test]
fn read_only_properties_stay_out() {
    let built = surface(false);
    let props = properties(&built);
    // Tagged ReadOnly: assigning it throws at runtime.
    assert!(!props["TextLabel"].contains_key("ContentText"));
    // GuiBase2d.AbsoluteSize and friends, likewise.
    assert!(!props["Frame"].contains_key("AbsoluteSize"));
    assert!(!props["Frame"].contains_key("AbsolutePosition"));
}

#[test]
fn capability_gated_properties_stay_out() {
    // Instance.Capabilities is writable as far as `Security` is concerned and
    // carries no ReadOnly tag. Only the `Capabilities` field rules it out, and
    // missing that field is how it reaches a props table.
    let built = surface(false);
    let props = properties(&built);
    assert!(!props["Frame"].contains_key("Capabilities"));
}

#[test]
fn elevated_security_properties_stay_out() {
    let built = surface(false);
    let props = properties(&built);
    // RobloxScriptSecurity on both read and write.
    assert!(!props["ScrollingFrame"].contains_key("SmoothScroll"));
    // PluginSecurity.
    assert!(!props["Frame"].contains_key("RobloxLocked"));
}

#[test]
fn hidden_is_reported_but_never_used_to_exclude() {
    // TextLabel.Font is tagged Hidden and remains the property most existing
    // code sets. Filtering on the tag would silently break that code.
    let surface = surface(false);
    let text_label = surface
        .classes
        .iter()
        .find(|c| c.name == "TextLabel")
        .expect("TextLabel");
    let font = text_label
        .properties
        .iter()
        .find(|p| p.name == "Font")
        .expect("TextLabel.Font is emitted");

    assert!(font.hidden);
    assert_eq!(font.luau, "Enum.Font");
}

#[test]
fn parent_is_left_to_react() {
    let built = surface(false);
    let props = properties(&built);
    for (class, fields) in &props {
        assert!(
            !fields.contains_key("Parent"),
            "{class} should not offer Parent as a prop"
        );
    }
}

#[test]
fn deprecated_properties_are_opt_in_and_counted() {
    let without = surface(false);
    let with = surface(true);

    assert!(without.skipped_deprecated > 0);
    assert_eq!(with.skipped_deprecated, 0);

    let lean = properties(&without);
    let full = properties(&with);
    // GuiObject.BackgroundColor is the BrickColor ancestor of BackgroundColor3.
    assert!(!lean["Frame"].contains_key("BackgroundColor"));
    assert!(full["Frame"].contains_key("BackgroundColor"));
}

#[test]
fn every_property_is_emitted_as_a_binding_union() {
    let surface = surface(false);
    let emitted = emit::emit(&surface, &emit::Style::default());

    for class in &surface.classes {
        for property in &class.properties {
            let expected = format!(
                "	{}: {}? | React.Binding<{}>?",
                property.name, property.luau, property.luau
            );
            assert!(
                emitted.source.contains(&expected),
                "missing binding union for {}.{}",
                class.name,
                property.name
            );
        }
    }
}

#[test]
fn generating_twice_gives_the_same_bytes() {
    let first = emit::emit(&surface(false), &emit::Style::default()).source;
    let second = emit::emit(&surface(false), &emit::Style::default()).source;
    assert_eq!(first, second);
}

#[test]
fn every_class_carries_the_indexer() {
    // Not an option. A strict variant was tried and removed: `React.Tag` is a
    // newproxy, and against a type with no indexer Luau unifies its key with
    // the declared string properties and reports a mismatch on an unrelated one
    // (`Property 'Active' is not compatible`). The indexer is what keeps every
    // prop marker react-lua ships usable.
    let surface = surface(false);
    let emitted = emit::emit(&surface, &emit::Style::default());

    // Counting emitted fields, not the phrase: the header note names the
    // indexer too, which is what makes a bare substring count off by one.
    let fields = emitted
        .source
        .lines()
        .filter(|line| line.trim() == "[any]: any,")
        .count();
    assert_eq!(fields, surface.classes.len());
}
