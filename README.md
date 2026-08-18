# react-luau-props

Luau prop types for every Roblox UI class, compiled from Roblox's own API dump
rather than written by hand.

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

**[Download `UiProps.luau`](https://raw.githubusercontent.com/rbx-forge/react-luau-props/main/generated/UiProps.luau)**
&nbsp;·&nbsp;
[browse it](generated/UiProps.luau)
&nbsp;·&nbsp;
[what it was built from](generated/manifest.json)

That one file is the whole story for almost everyone: put it in your project and
require it. The Rust crate here is not something you install. It exists to
produce the file and to prove, on every change, that the file is still what the
engine describes.

| You are | What you run |
| --- | --- |
| using the types in a game | nothing. Download `generated/UiProps.luau` |
| refreshing this repository after a Roblox release | `cargo run -- fetch`, then `cargo run -- generate` |

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
| no `Capabilities.Write` | `Instance.Capabilities` |
| not tagged `ReadOnly` or `NotScriptable` | `TextLabel.ContentText`, `GuiObject.AbsoluteSize` |

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

## The indexer

The generated types end with `[any]: any`. It is what makes the keys that are
not strings acceptable: `[React.Event.X]`, `[React.Change.X]` and `[React.Tag]`.

It costs one thing and only one: reading an undeclared field stops being an
error. Writing one was never checked either way, because Luau runs no
excess-property check of its own. Every property you do declare is still
checked as it was.

Your own component's props can ask for that check with `[string]: nil`. It
cannot apply here, since it rejects the three markers above.

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
URL=https://raw.githubusercontent.com/rbx-forge/react-luau-props/main/generated/UiProps.luau
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
