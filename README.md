# rbx-luau-props

Luau prop types for every Roblox UI class, compiled from Roblox's own API dump
rather than written by hand.

Two files, one derivation. They differ only in who is going to write the table.

**`UiProps.luau`** — one type per class, for a React `createElement` call:

```lua
local UiProps = require(ReplicatedStorage.Libraries.UiProps)

local function Card(props: { title: string, native: UiProps.Frame? })
    return e("Frame", joinDicts({ BackgroundTransparency = 1 }, props.native), {
        Title = e("TextLabel", {
            Text = props.title,
            FontFace = Font.fromEnum(Enum.Font.GothamBold),
            TextScaled = true,
        }),
    })
end
```

**`StyleRuleProps.luau`** — one flat type, for a `StyleRule`:

```lua
local StyleRuleProps = require(ReplicatedStorage.Libraries.StyleRuleProps)

local rule: StyleRuleProps.StyleRuleProps = {
    BackgroundColor3 = "$Surface",          -- a "$Token" reference, hence the widening
    Priority = 30,
    ["::UICorner"] = { CornerRadius = "$RadiusMd" },
    Transition = { BackgroundColor3 = TweenInfo.new(0.15) },
    BackgroundColour3 = Color3.new(),       -- a type error, and nothing else would catch it
}
```

A `StyleRule` names what it paints with a selector *string*, so no type can know
which class a rule targets: the flat union over the whole surface is the only
shape available. It closes with `[string]: nil`, which is the point — measured
against Studio, `SetProperties` accepts an unknown property name with no error
and no warning, and `GetProperties` shows it is stored. Nothing below the type
catches a typo, not the call, not the paint, and not a diff, since a rule's
properties live in a hidden `BinaryString`.

**[`UiProps.luau`](https://raw.githubusercontent.com/rbx-forge/rbx-luau-props/main/generated/UiProps.luau)**
&nbsp;·&nbsp;
**[`StyleRuleProps.luau`](https://raw.githubusercontent.com/rbx-forge/rbx-luau-props/main/generated/StyleRuleProps.luau)**
&nbsp;·&nbsp;
[what they were built from](generated/manifest.json)

Those files are the whole story for almost everyone: put one in your project and
require it. The Rust crate here is not something you install. It exists to
produce them and to prove, on every change, that they are still what the engine
describes.

| You are | What you run |
| --- | --- |
| using the types in a game | nothing. Download the file you need |
| refreshing this repository after a Roblox release | `cargo run -- fetch`, then `cargo run -- generate` |

Both files are always generated together. A flag would only create the state
where one is refreshed and the other silently is not, which is what `check`
exists to make impossible.

## What it is for

A React-Luau `createElement` call takes a table of Roblox properties, and
nothing checks it. Editor extensions offer completion from a hardcoded list of
names. This produces real Luau types instead, so the same file gives you
completion, checks the values you assign, and can be used as a data type
wherever props travel through your code.

Every property is emitted as `T? | React.Binding<T>?`, because react-roblox
tests for a binding on any key before assigning the property.

## Where the data comes from

Roblox publishes the deployed Studio version, and an API dump per deployment:

```
https://clientsettingscdn.roblox.com/v2/client-version/WindowsStudio64
https://setup.rbxcdn.com/{clientVersionUpload}-API-Dump.json
```

That dump is the engine describing itself. `fetch` vendors it into `vendor/`,
`generate` compiles it, and `generated/manifest.json` records which engine
release the output came from along with digests of both files.

Nothing in this crate lists classes or properties. The set is derived: a class
that descends from `GuiBase2d` or `UIBase` and is not `NotCreatable` is
emitted, with every property the engine says a script may assign. When Roblox
ships a new UI class, refreshing the dump picks it up.

### Which properties count as assignable

Four gates, each enforced separately by the engine, and missing any one of them
puts a property in your types that the engine will refuse at runtime:

| Gate | Excludes, for example |
| --- | --- |
| `MemberType` is `Property` | methods and events |
| `Security.Write` is `None` | `ScrollingFrame.SmoothScroll`, `Instance.RobloxLocked` |
| no `CapabilityControl` in `Capabilities.Write` | `Instance.Capabilities`, `Instance.Sandboxed` |
| not tagged `ReadOnly` or `NotScriptable` | `TextLabel.ContentText`, `GuiObject.AbsoluteSize` |

The capability gate is narrower than it first looks, and deliberately so.
`Capabilities` describes Roblox's *sandboxing* system: it names the capability a
script must hold, and a script outside a sandboxed container holds every
ordinary one. Treating any non-empty `Write` list as a gate was too strict, and
`StyleRule.Priority` is the counter-example — it carries `Write: ["UI"]`, and it
is assigned by working code. Only `CapabilityControl` really blocks, because it
governs the sandbox itself; the two properties carrying it are the two that have
no business in a props table anyway.

`Hidden` is deliberately *not* a gate. `TextLabel.Font` carries it while
remaining the property most existing code sets; filtering on the tag would
silently drop it. Hidden properties are emitted with a trailing comment
instead.

Deprecated properties are left out by default and counted in the manifest, so
the omission is visible rather than merely absent. `--include-deprecated` keeps
them, annotated.

`Parent` is the one name excluded by hand: React owns the hierarchy, and
setting it from props fights the reconciler.

## Refreshing the artifact

For maintainers of this repository only. Nothing below is needed to *use* the
types.

```sh
cargo run -- fetch      # vendor the dump for the deployed Studio
cargo run -- generate   # compile generated/UiProps.luau
cargo run -- check      # recompile and compare with what is committed
```

`check` is the guard that makes the committed file trustworthy: an edited
output, a half-refreshed dump, or a generator change nobody regenerated for all
fail there rather than reaching a consumer.

### Options

`--require` sets the expression the generated file requires React from,
defaulting to `ReplicatedStorage.Packages.React`.

## The indexers

The two files close differently, and the difference is the whole reason they are
two files.

`UiProps.luau` ends with `[any]: any`. It is what makes the keys that are not
strings acceptable: `[React.Event.X]`, `[React.Change.X]` and `[React.Tag]`.

It costs one thing and only one: reading an undeclared field stops being an
error. Writing one was never checked either way, because Luau runs no
excess-property check of its own. Every property you do declare is still checked
as it was.

`StyleRuleProps.luau` ends with `[string]: nil`, which is the check React cannot
have — a misspelled property becomes a type error. A `StyleRule` table carries
no non-string markers, so nothing stands in the way.

Two consequences follow from the strict form, both measured rather than assumed:

- It only works on a **flat** type. Inside an intersection, the member carrying
  `[string]: nil` demands that every string key be `nil`, including the ones its
  siblings declare, so even a valid property is rejected.
- A consumer therefore cannot add keys of its own by intersecting. That is why
  the rule's own properties and the `Transition` key are emitted into the file,
  and why both are read from the dump rather than written down: `Priority` is a
  real `StyleRule` property, and `Transition` is emitted only while the dump
  still declares `SetPropertyTransitions` behind it.

## Keeping it current

Run `fetch` again. The dump is always the currently deployed Studio, so there
is nothing to wait for. The `git diff` on `generated/UiProps.luau` is the
changelog: it tells you exactly which properties Roblox added or took away.

`check --upstream` answers the other question, the one no local file can:
whether Roblox has moved on since the vendored dump. Being behind is reported
rather than failed, because it usually means nothing. Over six Roblox releases
the UI surface gained one class and three properties.

A weekly job does this unattended and stays quiet unless the **types** change,
which is the only event worth a notification. When they do, it pushes a branch
and opens an issue carrying the property diff, leaving the pull request to a
human. Its schedule is read off Roblox's own deployment history rather than
guessed; the reasoning is in the workflow.

## Getting the file into a project

The artifact is committed, so `main` always holds the types for the latest
engine release this repository has been refreshed against. A one-line recipe is
usually the whole integration:

```sh
URL=https://raw.githubusercontent.com/rbx-forge/rbx-luau-props/main/generated/UiProps.luau
curl -fsSL "$URL" -o src/shared/UiProps.luau
```

There are no release tags. Swap `main` for a commit SHA to pin an older set,
though there is rarely a reason to: your experience runs on whatever Roblox
shipped today.

`generated/manifest.json` records the engine release and the digests, so a copy
found in a project can be traced back to what produced it.

### Formatting

The output is indented with tabs, which is StyLua's default, so a project that
has configured nothing leaves it alone. Pass `--indent spaces` if yours sets
`indent_type = "Spaces"`.

Either way, exclude it from your formatter. It is generated, and reformatting
it only creates a diff against every later download:

```
# .styluaignore
UiProps.luau
```

## Passing native properties through a component

Wrap a Roblox instance in a component and you need some way to let the caller
set the instance's own properties. Take a `native` field rather than widening
your component's props:

```lua
local function Pane(props: {
    native: UiProps.Frame?,
    children: React.ReactNode,
})
    return e("Frame", joinDicts({
        BackgroundTransparency = 1,
        BorderSizePixel = 0,
        Size = UDim2.fromScale(1, 1),
    }, props.native), props.children)
end

e(Pane, {
    padding = 5,
    native = { BackgroundColor3 = Color3.new(1, 1, 1) },
})
```

Widening instead means maintaining a list of your own fields to strip back out,
and a property Roblox adds later can collide with one of yours. That last one is
not hypothetical here: these types are recompiled from the engine, so the set
grows without anyone deciding it should. From [Kampfkarren's
guidelines](https://github.com/Kampfkarren/kampfkarren-luau-guidelines), which
type the passthrough as `{ [any]: any }?`; `UiProps.Frame?` says the same and
checks it.

For a one-off, an intersection works too, markers included:

```lua
type CardProps = UiProps.Frame & { title: string, onClose: () -> () }
```

## Trademark

Luau is a trademark of Roblox Corporation. This is an independent project, not
affiliated with or endorsed by Roblox or the Luau team.
