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

/// Properties the engine reports as assignable but that no React tree should set.
///
/// `Parent` is React's own business: setting it from props fights the
/// reconciler rather than the engine. There is deliberately nothing else here.
/// Every other exclusion is derived from what the dump says, so this list does
/// not quietly become the hand-maintained table the crate exists to replace.
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

#[derive(Debug, Clone, Copy)]
pub struct Options {
    /// Emit properties tagged `Deprecated`, annotated as such.
    pub include_deprecated: bool,
}

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

    fn descends_from_ui_root(&self, class: &str) -> bool {
        let mut current = class;
        loop {
            if ROOTS.contains(&current) {
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

    let mut names: Vec<&str> = dump
        .classes
        .iter()
        .filter(|class| {
            index.descends_from_ui_root(&class.name)
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
