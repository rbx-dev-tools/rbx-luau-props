//! The UI classes and properties this crate emits, derived from the dump.
//!
//! Nothing here is a list of class or property names. The set is derived: a
//! class that descends from a UI root and can be constructed is emitted, and a
//! property the engine says is assignable is emitted with it. When Roblox ships
//! a new UI class, refreshing the dump picks it up without an edit here.
//!
//! Every class is emitted flat, carrying its inherited properties rather than
//! composing them. `BillboardGui = LayerCollectorProps & GuiBase2dProps & ...`
//! is the obvious shape, cuts the file by three fifths, and does not work:
//! against an intersection Luau infers the props literal's own type first,
//! unifying its computed keys, and `[React.Tag] = "..."` (a newproxy holding a
//! string) will not reconcile with `[React.Event.X] = fn` (a table holding a
//! function). Both forms were generated and checked against the real packages;
//! composed fails on a tag alone and on a tag beside an event, flat passes
//! every marker react-lua ships. Duplication is what keeps them working, and
//! it is free in a file nobody writes by hand.

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};

use crate::dump::{Class, Dump};
use crate::ty::luau_type;

/// The two roots the UI surface hangs off. `GuiBase2d` covers `GuiObject` and
/// `LayerCollector` (`Frame`, `TextLabel`, `ScreenGui`, `SurfaceGui`...), `UIBase`
/// covers every `UIComponent` (layouts, padding, strokes, constraints).
pub const ROOTS: [&str; 2] = ["GuiBase2d", "UIBase"];

/// Which consumer a surface is compiled for.
///
/// The two want different class sets and different value widenings, but the
/// same derivation from the dump underneath. Nothing about a target is a list
/// of names: each one names roots and, where it needs to, a subtree to prune.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Target {
    /// React props: one type per class, every value widened with
    /// `React.Binding<T>`, closed with `[any]: any` so the prop markers work.
    #[default]
    React,
    /// `StyleRule` properties: one flat type over the whole surface, every
    /// value widened with `string` so a `"$Token"` reference type-checks,
    /// closed with `[string]: nil` so a misspelled property is an error.
    ///
    /// The flat shape is forced rather than chosen. `[string]: nil` inside an
    /// intersection makes the member carrying it demand that every string key
    /// be `nil`, including the ones its siblings declare, so even a valid
    /// property is rejected -- checked against `luau-lsp analyze`, not assumed.
    Style,
}

impl Target {
    /// The roots the surface hangs off.
    fn roots(self) -> &'static [&'static str] {
        match self {
            Target::React => &ROOTS,
            // A StyleRule also paints Path2D, which hangs off GuiBase rather
            // than GuiBase2d.
            Target::Style => &["GuiBase", "UIBase"],
        }
    }

    /// Subtrees under a root that this target does not want.
    fn pruned(self) -> &'static [&'static str] {
        match self {
            Target::React => &[],
            // GuiBase also roots the 3D adornments: handles, selection boxes,
            // wireframes. Rooting there instead of at GuiBase2d picks up
            // Path2D and 17 adornment classes, whose 28 further property names
            // (WireRadius, Humanoid, Velocity, TargetSurface...) no StyleRule
            // will ever paint. In a flat type closed with `[string]: nil`,
            // every extra name is one more typo that stops being caught -- so
            // the adornment subtree is pruned, and Path2D is what remains.
            Target::Style => &["GuiBase3d"],
        }
    }
}

/// Properties the engine reports as assignable but that no target should set.
///
/// `Parent` is out for both: setting it from React props fights the reconciler,
/// and a `StyleRule` cannot reparent what it paints at all. There is
/// deliberately nothing else here. Every other exclusion is derived from what
/// the dump says, so this list does not quietly become the hand-maintained
/// table the crate exists to replace.
const DENY: [&str; 1] = ["Parent"];

#[derive(Debug, Clone)]
pub struct Property {
    pub name: String,
    /// The class that declares it, used to group emitted fields the way the
    /// Studio properties panel groups them.
    pub owner: String,
    pub luau: String,
    pub deprecated: bool,
    pub hidden: bool,
}

#[derive(Debug, Clone)]
pub struct UiClass {
    pub name: String,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone)]
pub struct Surface {
    pub classes: Vec<UiClass>,
    /// Deprecated properties left out, counted so the omission is visible in
    /// the manifest rather than merely absent from the output.
    pub skipped_deprecated: usize,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Options {
    /// Emit properties tagged `Deprecated`, annotated as such.
    pub include_deprecated: bool,
    /// Which consumer the surface is compiled for.
    pub target: Target,
}

/// One property name as it appears in a flat, whole-surface type.
#[derive(Debug, Clone)]
pub struct FlatProperty {
    pub name: String,
    /// Every distinct Luau spelling this name carries across the surface. One
    /// name can mean different things on different classes -- `Transparency` is
    /// a number on a `GuiObject` and a `NumberSequence` on a `UIGradient` -- and
    /// a flat type has to accept all of them or reject a legitimate rule.
    pub luau: Vec<String>,
    /// The classes carrying it, so the emitted field can say where it comes
    /// from. A flat type loses the grouping the per-class shape had.
    pub owners: Vec<String>,
    pub hidden: bool,
}

/// The whole surface as a single type, for a target that cannot use one type
/// per class because the selector it is written against is a string.
#[derive(Debug, Clone)]
pub struct Flat {
    pub properties: Vec<FlatProperty>,
    /// The instance modifiers a rule may nest, written `::UICorner` and so on.
    /// Derived: every creatable `UIBase` descendant.
    pub modifiers: Vec<String>,
    /// `StyleRule`'s own assignable properties, which belong in a rule table
    /// alongside the properties of whatever the rule paints. `Priority` is the
    /// one that matters; it is read from the dump rather than written down.
    pub rule_properties: Vec<FlatProperty>,
    /// Whether the engine exposes per-property transitions on a `StyleRule`.
    ///
    /// Read from the dump rather than assumed: transitions are a real engine
    /// capability (`SetPropertyTransition`, `SetPropertyTransitions`,
    /// `SetDefaultPropertyTransition` and their getters), not something a
    /// wrapper invents. What a wrapper supplies is only the declarative
    /// spelling -- one `Transition` key unpacked into those calls -- so the key
    /// is emitted when, and only when, the engine still offers the methods.
    pub transitions: bool,
    pub skipped_deprecated: usize,
}

/// `StyleRule` properties that do not belong in a rule table.
///
/// `Selector` is the one: a declarative wrapper carries it as the table's key,
/// so a field of the same name would compete with it.
const RULE_DENY: [&str; 1] = ["Selector"];

/// The method whose presence means the engine still supports transitions.
const TRANSITION_METHOD: &str = "SetPropertyTransitions";

struct Index<'a> {
    by_name: BTreeMap<&'a str, &'a Class>,
}

impl<'a> Index<'a> {
    fn new(dump: &'a Dump) -> Self {
        Self {
            by_name: dump
                .classes
                .iter()
                .map(|class| (class.name.as_str(), class))
                .collect(),
        }
    }

    fn descends_from_any(&self, class: &str, roots: &[&str]) -> bool {
        let mut current = class;
        loop {
            if roots.contains(&current) {
                return true;
            }
            match self
                .by_name
                .get(current)
                .and_then(|c| c.superclass.as_deref())
            {
                Some(superclass) => current = superclass,
                None => return false,
            }
        }
    }

    /// The chain from the class itself up to `Instance`, leaf first.
    fn ancestry(&self, class: &str) -> Vec<&'a Class> {
        let mut chain = Vec::new();
        let mut current = Some(class);
        while let Some(name) = current {
            let Some(descriptor) = self.by_name.get(name) else {
                break;
            };
            chain.push(*descriptor);
            current = descriptor.superclass.as_deref();
        }
        chain
    }

    fn properties_of(
        &self,
        class: &str,
        options: Options,
        skipped_deprecated: &mut usize,
    ) -> Result<Vec<Property>> {
        let chain = self.ancestry(class);
        let mut seen = BTreeSet::new();
        let mut collected: Vec<Property> = Vec::new();

        // Leaf first, so a property a subclass redeclares keeps the subclass's
        // type. The ordering for output is applied afterwards.
        for descriptor in &chain {
            for member in &descriptor.members {
                if !member.is_assignable() || DENY.contains(&member.name.as_str()) {
                    continue;
                }
                if !seen.insert(member.name.clone()) {
                    continue;
                }

                let deprecated = member.is_deprecated();
                if deprecated && !options.include_deprecated {
                    *skipped_deprecated += 1;
                    continue;
                }

                let value_type = member
                    .value_type
                    .as_ref()
                    .with_context(|| format!("{class}.{} has no ValueType", member.name))?;
                let luau =
                    luau_type(value_type).with_context(|| format!("{class}.{}", member.name))?;

                collected.push(Property {
                    name: member.name.clone(),
                    owner: descriptor.name.clone(),
                    luau,
                    deprecated,
                    hidden: member.is_hidden(),
                });
            }
        }

        // Ancestors first, then alphabetical within each declaring class, so
        // the emitted type reads the shared properties before the ones that
        // make the class what it is.
        let depth = |owner: &str| self.ancestry(owner).len();
        collected.sort_by(|a, b| {
            depth(&b.owner)
                .cmp(&depth(&a.owner))
                .then_with(|| a.owner.cmp(&b.owner))
                .then_with(|| a.name.cmp(&b.name))
        });

        Ok(collected)
    }
}

pub fn build(dump: &Dump, options: Options) -> Result<Surface> {
    let index = Index::new(dump);

    let roots = options.target.roots();
    let pruned = options.target.pruned();

    let mut names: Vec<&str> = dump
        .classes
        .iter()
        .filter(|class| {
            index.descends_from_any(&class.name, roots)
                && !index.descends_from_any(&class.name, pruned)
                // Abstract bases (GuiObject, UIComponent...) exist to be
                // inherited from, never created by a React tree.
                && !class.tags.contains("NotCreatable")
        })
        .map(|class| class.name.as_str())
        .collect();
    names.sort_unstable();

    let mut skipped_deprecated = 0;
    let mut classes = Vec::new();
    for name in names {
        let properties = index.properties_of(name, options, &mut skipped_deprecated)?;
        if properties.is_empty() {
            continue;
        }
        classes.push(UiClass {
            name: name.to_owned(),
            properties,
        });
    }

    Ok(Surface {
        classes,
        skipped_deprecated,
    })
}

/// Collapse a surface into one type covering every class.
///
/// A `StyleRule` names what it paints with a selector string, so nothing in the
/// type system can know which class a given rule targets. One flat union over
/// the whole surface is the only shape available; it accepts `TextSize` on a
/// rule that only ever matches a `Frame`, and that imprecision is the price of
/// catching the misspellings, which is the thing nothing else catches.
pub fn flatten(dump: &Dump, options: Options) -> Result<Flat> {
    let surface = build(dump, options)?;
    let index = Index::new(dump);

    // BTreeMap so the output is ordered by name and stable between runs: the
    // committed file is compared byte for byte by `check`.
    let mut merged: BTreeMap<String, FlatProperty> = BTreeMap::new();

    for class in &surface.classes {
        for property in &class.properties {
            let entry = merged
                .entry(property.name.clone())
                .or_insert_with(|| FlatProperty {
                    name: property.name.clone(),
                    luau: Vec::new(),
                    owners: Vec::new(),
                    hidden: property.hidden,
                });

            if !entry.luau.contains(&property.luau) {
                entry.luau.push(property.luau.clone());
            }
            if !entry.owners.contains(&property.owner) {
                entry.owners.push(property.owner.clone());
            }
            // Hidden on any declaring class is worth reporting: the annotation
            // is a note to the reader, never a reason to leave a field out.
            entry.hidden = entry.hidden || property.hidden;
        }
    }

    for property in merged.values_mut() {
        property.luau.sort();
        property.owners.sort();
    }

    // The pseudo-instances a rule may nest are exactly the UI components a
    // sheet can create: creatable UIBase descendants. Derived like everything
    // else, so a new one Roblox ships arrives with the next dump refresh.
    let mut modifiers: Vec<String> = dump
        .classes
        .iter()
        .filter(|class| {
            index.descends_from_any(&class.name, &["UIBase"])
                && !class.tags.contains("NotCreatable")
        })
        .map(|class| class.name.clone())
        .collect();
    modifiers.sort();

    // The rule being built is itself an instance with properties. They are read
    // from the dump like everything else, minus the ones a declarative wrapper
    // owns structurally, and minus anything the painted surface already carries
    // (StyleRule inherits Name and Archivable from Instance, as every UI class
    // does, and one field cannot be declared twice).
    let mut rule_properties = Vec::new();
    let mut ignored = 0;
    for property in index.properties_of("StyleRule", options, &mut ignored)? {
        if RULE_DENY.contains(&property.name.as_str()) || merged.contains_key(&property.name) {
            continue;
        }
        rule_properties.push(FlatProperty {
            name: property.name,
            luau: vec![property.luau],
            owners: vec![property.owner],
            hidden: property.hidden,
        });
    }
    rule_properties.sort_by(|a, b| a.name.cmp(&b.name));

    let transitions = index.by_name.get("StyleRule").is_some_and(|class| {
        class
            .members
            .iter()
            .any(|member| member.member_type == "Function" && member.name == TRANSITION_METHOD)
    });

    Ok(Flat {
        properties: merged.into_values().collect(),
        modifiers,
        rule_properties,
        transitions,
        skipped_deprecated: surface.skipped_deprecated,
    })
}
