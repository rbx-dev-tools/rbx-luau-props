//! Rendering the surface as a Luau module.

use std::fmt::Write as _;

use crate::ir::{Flat, FlatProperty, Modifier, Surface};

/// The `React` module the generated file requires. Projects disagree about
/// where packages live, so the path is a parameter rather than a guess.
pub const DEFAULT_REQUIRE: &str = "ReplicatedStorage.Packages.React";

/// How the emitted body is indented.
///
/// This exists because a generated file a formatter wants to rewrite is a
/// recurring annoyance: the first `stylua` run produces a diff, and every later
/// refresh produces another. Matching the formatter is cheaper than fighting
/// it, so the default is `StyLua`'s own default rather than any one project's
/// taste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Indent {
    /// `StyLua`'s default, and so the one most projects will already agree with.
    #[default]
    Tabs,
    /// Four spaces, for projects that set `indent_type = "Spaces"`.
    Spaces,
}

impl Indent {
    fn as_str(self) -> &'static str {
        match self {
            Indent::Tabs => "\t",
            Indent::Spaces => "    ",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Style {
    pub require_path: String,
    pub indent: Indent,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            require_path: DEFAULT_REQUIRE.to_owned(),
            indent: Indent::default(),
        }
    }
}

pub struct Emitted {
    pub source: String,
    pub classes: usize,
    pub properties: usize,
}

/// Why the indexer is here, in the file itself, so the next person to read the
/// output does not have to rediscover it.
///
/// Prose inside a block comment, which no formatter reindents, so these keep
/// their own leading spaces whatever the body uses.
const INDEXER_NOTE: &[&str] = &[
    "    Every prop accepts its value or a React binding of that value:",
    "    react-roblox checks for a binding on any key before assigning the",
    "    property (RobloxComponentProps.applyProp).",
    "",
    "    The `[any]: any` indexer is what makes the keys that are not strings",
    "    acceptable: [React.Event.X], [React.Change.X] and [React.Tag] (a",
    "    space-separated string of tags). React's own string keys -- key, ref,",
    "    children, __self, __source -- need no indexer, because an undeclared",
    "    string key passes either way: Luau runs no excess-property check unless",
    "    a type asks for one, and this one cannot ask without rejecting the",
    "    markers above. A component's own props can: `[string]: nil`.",
    "",
    "    The indexer costs exactly one thing, measured rather than assumed:",
    "    reading an undeclared field stops being an error. Type checking on the",
    "    declared props survives it untouched.",
];

pub fn emit(surface: &Surface, style: &Style) -> Emitted {
    let mut out = String::new();
    let mut properties = 0;
    let tab = style.indent.as_str();

    write_header(&mut out, INDEXER_NOTE);

    let _ = writeln!(
        out,
        "local ReplicatedStorage = game:GetService(\"ReplicatedStorage\")\n"
    );
    let _ = writeln!(out, "local React = require({})\n", style.require_path);

    for class in &surface.classes {
        let _ = writeln!(out, "export type {} = {{", class.name);

        let mut last_owner: Option<&str> = None;
        for property in &class.properties {
            if last_owner != Some(property.owner.as_str()) {
                if last_owner.is_some() {
                    out.push('\n');
                }
                let _ = writeln!(out, "{tab}-- {}", property.owner);
                last_owner = Some(property.owner.as_str());
            }

            let mut note = String::new();
            if property.deprecated {
                note.push_str(" -- deprecated");
            }
            if property.hidden {
                note.push_str(if note.is_empty() {
                    " -- hidden"
                } else {
                    ", hidden"
                });
            }

            let _ = writeln!(
                out,
                "{tab}{}: {}? | React.Binding<{}>?,{}",
                property.name, property.luau, property.luau, note
            );
            properties += 1;
        }

        let _ = writeln!(out, "\n{tab}[any]: any,");
        out.push_str("}\n\n");
    }

    out.push_str("return nil\n");

    Emitted {
        source: out,
        classes: surface.classes.len(),
        properties,
    }
}

/// Why the flat shape and the closing indexer are what they are, carried in the
/// emitted file so the next reader does not have to rediscover it.
const STYLE_NOTE: &[&str] = &[
    "    A StyleRule names what it paints with a selector STRING, so nothing in",
    "    the type system can know which class a given rule targets. One flat",
    "    union over the whole surface is the only shape available. It accepts",
    "    TextSize on a rule that only ever matches a Frame, and that imprecision",
    "    is the price of catching the misspellings.",
    "",
    "    An instance modifier is the exception. `[\"::UICorner\"]` names exactly",
    "    ONE class, so each one gets a closed type of its own and a property",
    "    that is real but not on that class is an error inside it. The apology",
    "    above applies to the rule level only.",
    "",
    "    Every value is widened with `string`, because a styled property may",
    "    hold a \"$Token\" reference instead. The engine resolves those against",
    "    the sheet's attributes; no library interpolates them.",
    "",
    "    The file closes with `[string]: nil`, which is what makes a misspelled",
    "    property an error. That matters more here than anywhere else: measured",
    "    against Studio, SetProperties accepts an unknown property name with no",
    "    error and no warning, and GetProperties shows it is STORED. Nothing",
    "    below this type catches a typo -- not the call, not the paint, and not",
    "    a git diff, since a rule's properties live in a hidden BinaryString.",
    "",
    "    A Transition key is closed the same way, and for the same reason:",
    "    SetPropertyTransitions takes property NAMES too, so a misspelling",
    "    there is the same silent no-op. Every property is offered one, because",
    "    the dump does not say which values the engine can interpolate; a",
    "    TweenInfo on one it cannot is ignored rather than refused.",
    "",
    "    `[string]: nil` cannot be added by a consumer afterwards: inside an",
    "    intersection the member carrying it demands that every string key be",
    "    nil, including the ones its siblings declare, so even a valid property",
    "    is rejected. That is why the rule's own properties and the Transition",
    "    key are declared here rather than left for a wrapper to intersect on.",
    "",
    "    The two tables this module RETURNS are the same lists at runtime. A",
    "    builder has to know which keys configure the rule rather than name a",
    "    property, and which \"::\" names the engine can create; a hand-written",
    "    copy beside a generated list is exactly the drift this file removes.",
];

/// The wrapper's own key inside a `Transition` table, unpacked into
/// `SetDefaultPropertyTransition`.
const DEFAULT_TRANSITION: &str = "Default";

/// The union a styled value accepts.
fn styled_union(property: &FlatProperty) -> String {
    let mut union = property.luau.join(" | ");
    // A property already spelled `string` needs no widening: `string | Token`
    // is `string | string`, which reads as a mistake.
    let widened = !property.luau.iter().any(|luau| luau == "string");
    if widened {
        union.push_str(" | Token");
    }
    // Parentheses only where the union needs them. `(string)?` is legal and
    // reads like a leftover.
    if widened || property.luau.len() > 1 {
        union = format!("({union})");
    }
    union
}

/// Where a name comes from, for a reader of the flat type who has lost the
/// per-class grouping.
fn origin_note(property: &FlatProperty) -> String {
    let mut note = String::new();
    if property.hidden {
        note.push_str(" -- hidden");
    }
    if property.owners.len() > 1 {
        let _ = write!(
            note,
            "{} on {}",
            if note.is_empty() { " --" } else { "," },
            property.owners.join(", ")
        );
    }
    note
}

/// One `TweenInfo` per property name, closed.
fn write_transition_type(out: &mut String, tab: &str, name: &str, properties: &[FlatProperty]) {
    let _ = writeln!(out, "export type {name} = {{");
    for property in properties {
        let _ = writeln!(out, "{tab}{}: TweenInfo?,", property.name);
    }
    // Guarded rather than assumed: a dump that ever ships a property named
    // `Default` would otherwise declare the field twice, which does not
    // compile.
    if !properties
        .iter()
        .any(|property| property.name == DEFAULT_TRANSITION)
    {
        let _ = writeln!(
            out,
            "\n{tab}-- every property this rule paints and has no TweenInfo of its own"
        );
        let _ = writeln!(out, "{tab}{DEFAULT_TRANSITION}: TweenInfo?,");
    }
    let _ = writeln!(out, "\n{tab}[string]: nil,");
    out.push_str("}\n\n");
}

/// A modifier's own closed type, plus the transition type it points at.
fn write_modifier_type(
    out: &mut String,
    tab: &str,
    modifier: &Modifier,
    rule_properties: &[FlatProperty],
    transitions: bool,
) {
    let class = &modifier.class;
    let _ = writeln!(out, "export type {class} = {{");
    for property in &modifier.properties {
        let _ = writeln!(
            out,
            "{tab}{}: {}?,{}",
            property.name,
            styled_union(property),
            origin_note(property)
        );
    }

    // A modifier IS a StyleRule, so it carries the rule's own keys too -- minus
    // any the class already declares, since one field cannot be declared twice.
    let mut own: Vec<&FlatProperty> = rule_properties
        .iter()
        .filter(|rule| {
            !modifier
                .properties
                .iter()
                .any(|property| property.name == rule.name)
        })
        .collect();
    own.sort_by(|a, b| a.name.cmp(&b.name));

    if !own.is_empty() {
        out.push('\n');
        for property in own {
            let mut union = property.luau.join(" | ");
            if property.luau.len() > 1 {
                union = format!("({union})");
            }
            let _ = writeln!(out, "{tab}{}: {union}?,", property.name);
        }
    }

    if transitions {
        let _ = writeln!(out, "\n{tab}Transition: {class}Transition?,");
    }

    let _ = writeln!(out, "\n{tab}[string]: nil,");
    out.push_str("}\n\n");

    if transitions {
        write_transition_type(
            out,
            tab,
            &format!("{class}Transition"),
            &modifier.properties,
        );
    }
}

/// One runtime lookup table.
///
/// Deliberately NOT annotated `{ [string]: true }`. That was the first shape,
/// on the assumption that an indexer is what lets a builder test a key it
/// computed at runtime. Measured with `luau-lsp analyze`, it is not: iterating
/// the table, indexing it with `string.sub(key, 3)`, and indexing it with any
/// other computed string all type-check on the inferred type exactly as they do
/// on the annotated one. What the annotation does do is erase the key names,
/// which costs the two things worth having -- completion on `Modifiers.` and a
/// misspelling being an error rather than a silent `nil`. In a file that exists
/// to make a misspelled name an error, that is the wrong trade.
fn write_lookup(out: &mut String, tab: &str, name: &str, keys: impl Iterator<Item = String>) {
    let _ = writeln!(out, "local {name} = {{");
    for key in keys {
        let _ = writeln!(out, "{tab}{key} = true,");
    }
    out.push_str("}\n\n");
}

/// The preamble every emitted file carries: the warning, where it came from,
/// and the target's own note.
fn write_header(out: &mut String, note: &[&str]) {
    out.push_str("--!strict\n");
    out.push_str("--[[\n");
    out.push_str("    DO NOT EDIT BY HAND.\n");
    out.push('\n');
    out.push_str("    Generated by rbx-luau-props from Roblox's own API dump.\n");
    out.push_str("    https://github.com/rbx-dev-tools/rbx-luau-props\n");
    out.push('\n');
    for line in note {
        out.push_str(line);
        out.push('\n');
    }
    out.push_str("]]\n\n");
}

/// Render the flat surface as a single `StyleRuleProps` type.
pub fn emit_style(flat: &Flat, indent: Indent) -> Emitted {
    let mut out = String::new();
    let tab = indent.as_str();
    let mut properties = 0;

    write_header(&mut out, STYLE_NOTE);

    out.push_str("-- A \"$Name\" reference to a token held as an attribute on the StyleSheet.\n");
    out.push_str("export type Token = string\n\n");

    out.push_str("export type StyleRuleProps = {\n");

    for property in &flat.properties {
        let _ = writeln!(
            out,
            "{tab}{}: {}?,{}",
            property.name,
            styled_union(property),
            origin_note(property)
        );
        properties += 1;
    }

    out.push('\n');
    let _ = writeln!(
        out,
        "{tab}-- instance modifiers: every creatable UIBase descendant. Each one is\n{tab}-- built as a child StyleRule whose selector is the key, and its type is\n{tab}-- the only precise one in this file."
    );
    for modifier in &flat.modifiers {
        let _ = writeln!(out, "{tab}[\"::{0}\"]: {0}?,", modifier.class);
    }

    // Both of these are declared here rather than left to a wrapper because a
    // type closed with `[string]: nil` cannot be extended through an
    // intersection, and both are read from the dump rather than written down.
    if !flat.rule_properties.is_empty() {
        out.push('\n');
        let _ = writeln!(
            out,
            "{tab}-- the rule's own properties. NOT widened with Token: a \"$Name\"\n{tab}-- reference is resolved by the engine for a STYLED property, the kind\n{tab}-- that goes in through SetProperties. These are properties of the rule\n{tab}-- instance itself, assigned directly, and a string would fail the cast."
        );
        for property in &flat.rule_properties {
            let mut union = property.luau.join(" | ");
            if property.luau.len() > 1 {
                union = format!("({union})");
            }
            let _ = writeln!(out, "{tab}{}: {union}?,", property.name);
            properties += 1;
        }
    }

    if flat.transitions {
        out.push('\n');
        let _ = writeln!(
            out,
            "{tab}-- the declarative spelling of SetPropertyTransitions and\n{tab}-- SetDefaultPropertyTransition, which the dump still declares"
        );
        let _ = writeln!(out, "{tab}Transition: StyleRuleTransition?,");
    }

    out.push('\n');
    let _ = writeln!(out, "{tab}[string]: nil,");
    out.push_str("}\n\n");

    if flat.transitions {
        write_transition_type(&mut out, tab, "StyleRuleTransition", &flat.properties);
    }

    for modifier in &flat.modifiers {
        write_modifier_type(
            &mut out,
            tab,
            modifier,
            &flat.rule_properties,
            flat.transitions,
        );
    }

    out.push_str("-- Which keys configure the rule rather than naming a property it paints.\n");
    let mut rule_keys: Vec<String> = flat
        .rule_properties
        .iter()
        .map(|property| property.name.clone())
        .collect();
    if flat.transitions {
        rule_keys.push("Transition".to_owned());
    }
    rule_keys.sort();
    write_lookup(&mut out, tab, "RuleKeys", rule_keys.into_iter());

    out.push_str("-- The \"::\" names the engine can create, without the prefix.\n");
    write_lookup(
        &mut out,
        tab,
        "Modifiers",
        flat.modifiers.iter().map(|modifier| modifier.class.clone()),
    );

    out.push_str("return {\n");
    let _ = writeln!(out, "{tab}RuleKeys = RuleKeys,");
    let _ = writeln!(out, "{tab}Modifiers = Modifiers,");
    out.push_str("}\n");

    Emitted {
        source: out,
        classes: 1,
        properties,
    }
}
