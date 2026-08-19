use std::{env, path::PathBuf};

use anyhow::Result;
use clap::{Args, Parser, Subcommand, ValueEnum};
use rbx_luau_props::{check, emit, fetch, find_root, generate, write, Config, OUTPUT, TYPES};

#[derive(Debug, Parser)]
#[command(name = "rbx-luau-props", version, about)]
struct Cli {
    /// Repository root. Defaults to the nearest directory holding Cargo.toml.
    #[arg(long, global = true)]
    root: Option<PathBuf>,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Args)]
struct Shape {
    /// Luau expression the generated file requires React from.
    #[arg(long, default_value = emit::DEFAULT_REQUIRE)]
    require: String,

    /// Emit properties tagged `Deprecated`, annotated as such.
    #[arg(long)]
    include_deprecated: bool,

    /// Indentation of the emitted body.
    ///
    /// Defaults to tabs, which is `StyLua`'s default, so a project that has not
    /// configured its formatter leaves the file alone. Use `spaces` for a
    /// project that sets `indent_type = "Spaces"`.
    #[arg(long, value_enum, default_value_t = IndentArg::Tabs)]
    indent: IndentArg,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum IndentArg {
    Tabs,
    Spaces,
}

impl From<IndentArg> for emit::Indent {
    fn from(arg: IndentArg) -> Self {
        match arg {
            IndentArg::Tabs => emit::Indent::Tabs,
            IndentArg::Spaces => emit::Indent::Spaces,
        }
    }
}

impl From<&Shape> for Config {
    fn from(shape: &Shape) -> Self {
        Self {
            style: emit::Style {
                require_path: shape.require.clone(),
                indent: shape.indent.into(),
            },
            include_deprecated: shape.include_deprecated,
        }
    }
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Compile the types and write them into generated/.
    Generate {
        #[command(flatten)]
        shape: Shape,
    },
    /// Recompile and compare with what is committed.
    Check {
        #[command(flatten)]
        shape: Shape,

        /// Also ask Roblox whether the vendored dump is still the deployed one.
        ///
        /// Being behind is reported, not failed: Roblox ships a version most
        /// weeks and the UI surface rarely moves with it. What matters is
        /// whether the types would change, which only regenerating can say.
        #[arg(long)]
        upstream: bool,
    },
    /// Download the API dump for the currently deployed Studio.
    Fetch,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let root = match cli.root {
        Some(root) => root,
        None => find_root(&env::current_dir()?)?,
    };

    match cli.command {
        Command::Generate { shape } => {
            let artifacts = generate(&root, &Config::from(&shape))?;
            write(&root, &artifacts)?;
            println!(
                "wrote {OUTPUT}/{TYPES}: {} classes, {} properties, from Roblox {}",
                artifacts.manifest.classes,
                artifacts.manifest.properties,
                artifacts.manifest.roblox_version
            );
            if artifacts.manifest.skipped_deprecated > 0 {
                println!(
                    "{} deprecated properties left out; --include-deprecated keeps them",
                    artifacts.manifest.skipped_deprecated
                );
            }
        }
        Command::Check { shape, upstream } => {
            check(&root, &Config::from(&shape))?;
            println!("generated files are current");

            if upstream {
                match rbx_luau_props::behind_upstream(&root)? {
                    None => println!("the vendored dump is the deployed Studio"),
                    Some((vendored, deployed)) => println!(
                        "Roblox is on {deployed}, the vendored dump is {vendored};                          run `fetch` then `generate` to see whether the types move"
                    ),
                }
            }
        }
        Command::Fetch => {
            let (version, upload) = fetch(&root)?;
            println!("vendored the API dump for Roblox {version} ({upload})");
            println!("run `rbx-luau-props generate` next, then review the diff");
        }
    }

    Ok(())
}
