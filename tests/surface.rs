//! Tests over the vendored dump, so they assert the thing that matters: the
//! properties the engine really exposes end up in the file, and the ones it
//! refuses to let a script assign stay out of it.

use rbx_luau_props::{dump, emit, ir};
use std::{collections::BTreeMap, fs, path::PathBuf};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn surface(include_deprecated: bool) -> ir::Surface {
    let bytes = fs::read(root().join(rbx_luau_props::VENDORED_DUMP))
        .expect("vendored dump; run `rbx-luau-props fetch`");
    let parsed = dump::parse(&bytes).expect("the vendored dump parses");
    ir::build(
        &parsed,
        ir::Options {
            include_deprecated,
            target: ir::Target::React,
        },
    )
    .expect("the surface builds")
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

// --- the styling target -------------------------------------------------
//
// A StyleRule is written against a selector string, so it gets one flat type
// over the whole surface rather than one per class. What follows pins the four
// things that shape is for.

fn flat(include_deprecated: bool) -> ir::Flat {
    let bytes = fs::read(root().join(rbx_luau_props::VENDORED_DUMP))
        .expect("vendored dump; run `rbx-luau-props fetch`");
    let parsed = dump::parse(&bytes).expect("the vendored dump parses");
    ir::flatten(
        &parsed,
        ir::Options {
            include_deprecated,
            target: ir::Target::Style,
        },
    )
    .expect("the flat surface builds")
}

fn flat_names(flat: &ir::Flat) -> BTreeMap<&str, &ir::FlatProperty> {
    flat.properties
        .iter()
        .map(|p| (p.name.as_str(), p))
        .collect()
}

#[test]
fn the_styling_target_reaches_path2d() {
    // The whole reason it roots at GuiBase rather than GuiBase2d. Both are
    // assignable, and the React target misses them.
    let built = flat(false);
    let names = flat_names(&built);
    assert!(names.contains_key("Closed"), "Path2D.Closed is missing");
    assert!(names.contains_key("Color3"), "Path2D.Color3 is missing");
}

#[test]
fn the_styling_target_prunes_the_3d_adornments() {
    // GuiBase also roots the handles and selection boxes. In a type closed
    // with `[string]: nil` every name it carries is a typo that stops being
    // caught, and no StyleRule paints a wireframe.
    let built = flat(false);
    let names = flat_names(&built);
    for property in [
        "WireRadius",
        "Humanoid",
        "Velocity",
        "TargetSurface",
        "AdornCullingMode",
    ] {
        assert!(
            !names.contains_key(property),
            "{property} comes from the 3D adornments and should be pruned"
        );
    }
}

#[test]
fn a_name_meaning_two_things_keeps_both() {
    // Transparency is a number on a GuiObject and a NumberSequence on a
    // UIGradient. A flat type that kept only one would reject a legitimate
    // rule, so the union carries every spelling the surface uses.
    let built = flat(false);
    let names = flat_names(&built);
    let transparency = names["Transparency"];
    assert!(transparency.luau.contains(&"number".to_owned()));
    assert!(transparency.luau.contains(&"NumberSequence".to_owned()));
    assert!(transparency.owners.len() > 1);
}

#[test]
fn the_modifiers_are_the_creatable_ui_components() {
    let flat = flat(false);
    for modifier in ["UICorner", "UIStroke", "UIGradient", "UIPadding"] {
        assert!(
            flat.modifiers.iter().any(|m| m == modifier),
            "{modifier} should be an instance modifier"
        );
    }
    // Abstract, so it can never be created as a pseudo-instance.
    assert!(!flat.modifiers.iter().any(|m| m == "UIComponent"));
    assert!(!flat.modifiers.iter().any(|m| m == "UIBase"));
}

#[test]
fn every_styled_value_admits_a_token_reference() {
    let flat = flat(false);
    let emitted = emit::emit_style(&flat, emit::Indent::default());

    for property in &flat.properties {
        let line = emitted
            .source
            .lines()
            .find(|line| {
                line.trim_start()
                    .starts_with(&format!("{}: ", property.name))
            })
            .unwrap_or_else(|| panic!("{} is not emitted", property.name));

        // Either it is widened with Token, or it is already a string and
        // widening it would read as a mistake.
        assert!(
            line.contains("| Token)") || property.luau.iter().any(|luau| luau == "string"),
            "{} is not open to a \"$Token\" reference: {line}",
            property.name
        );
    }
}

#[test]
fn the_styling_type_is_closed() {
    // This is the point of the target: without it a misspelled property is
    // accepted by the type checker, and the engine does not guard it either.
    let emitted = emit::emit_style(&flat(false), emit::Indent::default());
    assert!(emitted.source.contains("[string]: nil,"));
    assert!(!emitted.source.contains("[any]: any,"));
}

#[test]
fn the_styling_type_carries_the_rules_own_keys() {
    // A closed type cannot be extended through an intersection: the member
    // holding `[string]: nil` demands every string key be nil, including the
    // ones its siblings declare. So these have to be emitted here -- and both
    // are read from the dump rather than written down.
    let built = flat(false);
    let emitted = emit::emit_style(&built, emit::Indent::default());

    // StyleRule.Priority is a real property, with the type the engine gives it.
    assert!(built.rule_properties.iter().any(|p| p.name == "Priority"));

    // And NOT widened with Token, unlike everything the rule paints. A "$Name"
    // reference is resolved by the engine for a styled property, the kind that
    // goes in through SetProperties. The rule's own properties are assigned
    // directly onto the instance, so a string would fail the cast at runtime --
    // a type that accepted one would permit code that cannot work.
    assert!(emitted.source.contains("Priority: number?,"));
    assert!(!emitted.source.contains("Priority: (number | Token)?,"));

    // Selector is the one StyleRule property a declarative wrapper owns
    // structurally: it is the table's key, so a field would compete with it.
    assert!(!built.rule_properties.iter().any(|p| p.name == "Selector"));
    assert!(!emitted.source.contains("Selector:"));

    // Transitions are an engine capability, not a wrapper invention. The key is
    // emitted because the dump still declares the methods behind it.
    assert!(built.transitions);
    assert!(emitted
        .source
        .contains("Transition: { [string]: TweenInfo }?,"));
}

#[test]
fn the_rules_own_keys_never_collide_with_the_painted_surface() {
    // StyleRule inherits Name and Archivable from Instance, and so does every
    // UI class the surface already carries. Declaring a field twice does not
    // compile, so the rule's properties are only added where the surface has
    // no field of that name.
    let built = flat(false);
    let painted: BTreeMap<&str, &ir::FlatProperty> = flat_names(&built);

    for property in &built.rule_properties {
        assert!(
            !painted.contains_key(property.name.as_str()),
            "{} is declared twice",
            property.name
        );
    }
}

#[test]
fn parent_is_left_out_of_the_styling_target_too() {
    let built = flat(false);
    let names = flat_names(&built);
    assert!(!names.contains_key("Parent"));
}

#[test]
fn the_styling_target_needs_no_react() {
    let emitted = emit::emit_style(&flat(false), emit::Indent::default());
    assert!(!emitted.source.contains("React"));
    assert!(!emitted.source.contains("Binding"));
}

#[test]
fn generating_the_styling_target_twice_gives_the_same_bytes() {
    let first = emit::emit_style(&flat(false), emit::Indent::default()).source;
    let second = emit::emit_style(&flat(false), emit::Indent::default()).source;
    assert_eq!(first, second);
}
